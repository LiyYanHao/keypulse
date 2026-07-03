use chrono::{Local, Timelike};
#[cfg(target_os = "macos")]
use core_foundation::runloop::CFRunLoop;
#[cfg(target_os = "macos")]
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField, KeyCode,
};
use directories::ProjectDirs;
#[cfg(not(target_os = "macos"))]
use rdev::{listen, Event, EventType};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    ActivationPolicy, AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};

const STATS_EVENT: &str = "stats-updated";

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KeyStats {
    date: String,
    started_at: String,
    updated_at: String,
    total_keys: u64,
    current_minute_keys: u64,
    peak_per_minute: u64,
    category_counts: HashMap<String, u64>,
    shortcut_counts: HashMap<String, u64>,
    #[serde(default = "default_hourly_counts")]
    hourly_counts: [u64; 24],
    #[serde(default = "default_half_hourly_counts")]
    half_hourly_counts: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct StatsSnapshot {
    listening: bool,
    input_monitoring_granted: bool,
    permission_hint: String,
    storage_path: String,
    stats: KeyStats,
    history_days: Vec<HeatmapDay>,
    top_shortcuts: Vec<ShortcutCount>,
    top_categories: Vec<CategoryCount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HeatmapDay {
    date: String,
    total_keys: u64,
    hourly_counts: [u64; 24],
    half_hourly_counts: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutCount {
    shortcut: String,
    count: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CategoryCount {
    category: String,
    count: u64,
}

#[derive(Debug, Default)]
struct Modifiers {
    cmd: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

struct RuntimeState {
    stats: KeyStats,
    history: HashMap<String, KeyStats>,
    recent_events: VecDeque<u128>,
    modifiers: Modifiers,
    last_save_ms: u128,
}

struct KeyPulseState {
    runtime: Arc<Mutex<RuntimeState>>,
    running: Arc<AtomicBool>,
    listener_started: AtomicBool,
    tray_icon: Mutex<Option<TrayIcon>>,
    storage_path: PathBuf,
    history_path: PathBuf,
}

fn today_string() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn now_string() -> String {
    Local::now().to_rfc3339()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0))
        .as_millis()
}

fn default_category_counts() -> HashMap<String, u64> {
    [
        "ordinary",
        "letter",
        "number",
        "enter",
        "backspace",
        "tab",
        "escape",
        "arrow",
        "function",
        "modifier",
        "shortcut",
        "other",
    ]
    .iter()
    .map(|item| ((*item).to_string(), 0))
    .collect()
}

fn default_hourly_counts() -> [u64; 24] {
    [0; 24]
}

fn default_half_hourly_counts() -> Vec<u64> {
    vec![0; 48]
}

fn fresh_stats() -> KeyStats {
    let now = now_string();
    KeyStats {
        date: today_string(),
        started_at: now.clone(),
        updated_at: now,
        total_keys: 0,
        current_minute_keys: 0,
        peak_per_minute: 0,
        category_counts: default_category_counts(),
        shortcut_counts: HashMap::new(),
        hourly_counts: default_hourly_counts(),
        half_hourly_counts: default_half_hourly_counts(),
    }
}

fn stats_dir() -> PathBuf {
    ProjectDirs::from("cn", "xingshi", "KeyPulse")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("keypulse"))
}

fn stats_path() -> PathBuf {
    stats_dir().join("today-stats.json")
}

fn history_path() -> PathBuf {
    stats_dir().join("stats-history.json")
}

fn normalize_stats(stats: &mut KeyStats) {
    for (key, value) in default_category_counts() {
        stats.category_counts.entry(key).or_insert(value);
    }
    stats.half_hourly_counts.resize(48, 0);
    stats.half_hourly_counts.truncate(48);
    let hourly_total: u64 = stats.hourly_counts.iter().sum();
    let half_hourly_total: u64 = stats.half_hourly_counts.iter().sum();
    if hourly_total > 0 && half_hourly_total == 0 {
        for (hour, count) in stats.hourly_counts.iter().enumerate() {
            let first_half = count.saturating_sub(count / 2);
            stats.half_hourly_counts[hour * 2] = first_half;
            stats.half_hourly_counts[hour * 2 + 1] = count / 2;
        }
    }
}

fn load_stats(path: &PathBuf) -> KeyStats {
    let Ok(raw) = fs::read_to_string(path) else {
        return fresh_stats();
    };
    let Ok(mut stats) = serde_json::from_str::<KeyStats>(&raw) else {
        return fresh_stats();
    };
    if stats.date != today_string() {
        return fresh_stats();
    }
    normalize_stats(&mut stats);
    stats.current_minute_keys = 0;
    stats
}

fn load_history(path: &PathBuf) -> HashMap<String, KeyStats> {
    let Ok(raw) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(mut history) = serde_json::from_str::<HashMap<String, KeyStats>>(&raw) else {
        return HashMap::new();
    };
    for stats in history.values_mut() {
        normalize_stats(stats);
        stats.current_minute_keys = 0;
    }
    history
}

fn save_stats(path: &PathBuf, stats: &KeyStats) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(payload) = serde_json::to_string_pretty(stats) {
        let _ = fs::write(path, payload);
    }
}

fn save_history(path: &PathBuf, history: &HashMap<String, KeyStats>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(payload) = serde_json::to_string_pretty(history) {
        let _ = fs::write(path, payload);
    }
}

fn is_modifier(key: &str) -> bool {
    key.contains("Meta") || key.contains("Control") || key.contains("Alt") || key.contains("Shift")
}

fn update_modifier(modifiers: &mut Modifiers, key: &str, down: bool) {
    if key.contains("Meta") {
        modifiers.cmd = down;
    } else if key.contains("Control") {
        modifiers.ctrl = down;
    } else if key.contains("Alt") {
        modifiers.alt = down;
    } else if key.contains("Shift") {
        modifiers.shift = down;
    }
}

fn modifier_is_active(modifiers: &Modifiers, key: &str) -> bool {
    if key.contains("Meta") {
        modifiers.cmd
    } else if key.contains("Control") {
        modifiers.ctrl
    } else if key.contains("Alt") {
        modifiers.alt
    } else if key.contains("Shift") {
        modifiers.shift
    } else {
        false
    }
}

fn visible_key_name(key: &str) -> String {
    if let Some(letter) = key.strip_prefix("Key") {
        if letter.len() == 1 && letter.chars().all(|c| c.is_ascii_alphabetic()) {
            return letter.to_ascii_uppercase();
        }
    }
    if let Some(number) = key.strip_prefix("Num") {
        if number.len() == 1 && number.chars().all(|c| c.is_ascii_digit()) {
            return number.to_string();
        }
    }
    match key {
        "Return" => "Enter".to_string(),
        "Backspace" => "Backspace".to_string(),
        "Escape" => "Esc".to_string(),
        "Tab" => "Tab".to_string(),
        "Space" => "Space".to_string(),
        "LeftArrow" => "Left".to_string(),
        "RightArrow" => "Right".to_string(),
        "UpArrow" => "Up".to_string(),
        "DownArrow" => "Down".to_string(),
        other => other.to_string(),
    }
}

fn shortcut_label(modifiers: &Modifiers, key: &str) -> Option<String> {
    if is_modifier(key) {
        return None;
    }
    if !(modifiers.cmd || modifiers.ctrl || modifiers.alt) {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    if modifiers.cmd {
        parts.push("Cmd".to_string());
    }
    if modifiers.ctrl {
        parts.push("Ctrl".to_string());
    }
    if modifiers.alt {
        parts.push("Alt".to_string());
    }
    if modifiers.shift {
        parts.push("Shift".to_string());
    }
    parts.push(visible_key_name(key));
    Some(parts.join("+"))
}

fn category_for_key(key: &str) -> &'static str {
    if is_modifier(key) {
        return "modifier";
    }
    if key.starts_with("Key") {
        return "letter";
    }
    if key.starts_with("Num") {
        return "number";
    }
    match key {
        "Return" => "enter",
        "Backspace" => "backspace",
        "Tab" => "tab",
        "Escape" => "escape",
        "LeftArrow" | "RightArrow" | "UpArrow" | "DownArrow" => "arrow",
        _ if key.starts_with('F') && key[1..].chars().all(|c| c.is_ascii_digit()) => "function",
        _ => "other",
    }
}

#[cfg(target_os = "macos")]
fn mac_key_name(keycode: u16) -> &'static str {
    match keycode {
        KeyCode::ANSI_A => "KeyA",
        KeyCode::ANSI_B => "KeyB",
        KeyCode::ANSI_C => "KeyC",
        KeyCode::ANSI_D => "KeyD",
        KeyCode::ANSI_E => "KeyE",
        KeyCode::ANSI_F => "KeyF",
        KeyCode::ANSI_G => "KeyG",
        KeyCode::ANSI_H => "KeyH",
        KeyCode::ANSI_I => "KeyI",
        KeyCode::ANSI_J => "KeyJ",
        KeyCode::ANSI_K => "KeyK",
        KeyCode::ANSI_L => "KeyL",
        KeyCode::ANSI_M => "KeyM",
        KeyCode::ANSI_N => "KeyN",
        KeyCode::ANSI_O => "KeyO",
        KeyCode::ANSI_P => "KeyP",
        KeyCode::ANSI_Q => "KeyQ",
        KeyCode::ANSI_R => "KeyR",
        KeyCode::ANSI_S => "KeyS",
        KeyCode::ANSI_T => "KeyT",
        KeyCode::ANSI_U => "KeyU",
        KeyCode::ANSI_V => "KeyV",
        KeyCode::ANSI_W => "KeyW",
        KeyCode::ANSI_X => "KeyX",
        KeyCode::ANSI_Y => "KeyY",
        KeyCode::ANSI_Z => "KeyZ",
        KeyCode::ANSI_0 | KeyCode::ANSI_KEYPAD_0 => "Num0",
        KeyCode::ANSI_1 | KeyCode::ANSI_KEYPAD_1 => "Num1",
        KeyCode::ANSI_2 | KeyCode::ANSI_KEYPAD_2 => "Num2",
        KeyCode::ANSI_3 | KeyCode::ANSI_KEYPAD_3 => "Num3",
        KeyCode::ANSI_4 | KeyCode::ANSI_KEYPAD_4 => "Num4",
        KeyCode::ANSI_5 | KeyCode::ANSI_KEYPAD_5 => "Num5",
        KeyCode::ANSI_6 | KeyCode::ANSI_KEYPAD_6 => "Num6",
        KeyCode::ANSI_7 | KeyCode::ANSI_KEYPAD_7 => "Num7",
        KeyCode::ANSI_8 | KeyCode::ANSI_KEYPAD_8 => "Num8",
        KeyCode::ANSI_9 | KeyCode::ANSI_KEYPAD_9 => "Num9",
        KeyCode::RETURN | KeyCode::ANSI_KEYPAD_ENTER => "Return",
        KeyCode::DELETE | KeyCode::FORWARD_DELETE => "Backspace",
        KeyCode::TAB => "Tab",
        KeyCode::ESCAPE => "Escape",
        KeyCode::SPACE => "Space",
        KeyCode::LEFT_ARROW => "LeftArrow",
        KeyCode::RIGHT_ARROW => "RightArrow",
        KeyCode::UP_ARROW => "UpArrow",
        KeyCode::DOWN_ARROW => "DownArrow",
        KeyCode::F1 => "F1",
        KeyCode::F2 => "F2",
        KeyCode::F3 => "F3",
        KeyCode::F4 => "F4",
        KeyCode::F5 => "F5",
        KeyCode::F6 => "F6",
        KeyCode::F7 => "F7",
        KeyCode::F8 => "F8",
        KeyCode::F9 => "F9",
        KeyCode::F10 => "F10",
        KeyCode::F11 => "F11",
        KeyCode::F12 => "F12",
        KeyCode::F13 => "F13",
        KeyCode::F14 => "F14",
        KeyCode::F15 => "F15",
        KeyCode::F16 => "F16",
        KeyCode::F17 => "F17",
        KeyCode::F18 => "F18",
        KeyCode::F19 => "F19",
        KeyCode::F20 => "F20",
        KeyCode::COMMAND => "MetaLeft",
        KeyCode::RIGHT_COMMAND => "MetaRight",
        KeyCode::CONTROL => "ControlLeft",
        KeyCode::RIGHT_CONTROL => "ControlRight",
        KeyCode::OPTION => "AltLeft",
        KeyCode::RIGHT_OPTION => "AltRight",
        KeyCode::SHIFT => "ShiftLeft",
        KeyCode::RIGHT_SHIFT => "ShiftRight",
        _ => "Other",
    }
}

#[cfg(target_os = "macos")]
fn mac_modifier_is_down(flags: CGEventFlags, key: &str) -> bool {
    if key.contains("Meta") {
        flags.contains(CGEventFlags::CGEventFlagCommand)
    } else if key.contains("Control") {
        flags.contains(CGEventFlags::CGEventFlagControl)
    } else if key.contains("Alt") {
        flags.contains(CGEventFlags::CGEventFlagAlternate)
    } else if key.contains("Shift") {
        flags.contains(CGEventFlags::CGEventFlagShift)
    } else {
        false
    }
}

#[cfg(target_os = "macos")]
fn mac_listen_event_access_granted() -> bool {
    unsafe { CGPreflightListenEventAccess() || CGRequestListenEventAccess() }
}

fn input_monitoring_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe { CGPreflightListenEventAccess() }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

fn bump(map: &mut HashMap<String, u64>, key: &str) {
    *map.entry(key.to_string()).or_insert(0) += 1;
}

fn sorted_counts(map: &HashMap<String, u64>, limit: usize) -> Vec<(String, u64)> {
    let mut items: Vec<(String, u64)> = map
        .iter()
        .filter(|(_, count)| **count > 0)
        .map(|(key, count)| (key.clone(), *count))
        .collect();
    items.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    items.truncate(limit);
    items
}

fn history_days(runtime: &RuntimeState) -> Vec<HeatmapDay> {
    let mut by_date = runtime.history.clone();
    by_date.insert(runtime.stats.date.clone(), runtime.stats.clone());
    let mut days: Vec<HeatmapDay> = by_date
        .values()
        .map(|stats| HeatmapDay {
            date: stats.date.clone(),
            total_keys: stats.total_keys,
            hourly_counts: stats.hourly_counts,
            half_hourly_counts: stats.half_hourly_counts.clone(),
        })
        .collect();
    days.sort_by(|left, right| left.date.cmp(&right.date));
    days
}

fn persist_runtime(state: &KeyPulseState, runtime: &mut RuntimeState) {
    runtime
        .history
        .insert(runtime.stats.date.clone(), runtime.stats.clone());
    save_stats(&state.storage_path, &runtime.stats);
    save_history(&state.history_path, &runtime.history);
}

fn snapshot_from_runtime(
    runtime: &RuntimeState,
    running: bool,
    storage_path: &PathBuf,
) -> StatsSnapshot {
    StatsSnapshot {
        listening: running,
        input_monitoring_granted: input_monitoring_granted(),
        permission_hint: permission_hint(),
        storage_path: storage_path.display().to_string(),
        stats: runtime.stats.clone(),
        history_days: history_days(runtime),
        top_shortcuts: sorted_counts(&runtime.stats.shortcut_counts, 8)
            .into_iter()
            .map(|(shortcut, count)| ShortcutCount { shortcut, count })
            .collect(),
        top_categories: sorted_counts(&runtime.stats.category_counts, 12)
            .into_iter()
            .map(|(category, count)| CategoryCount { category, count })
            .collect(),
    }
}

fn reset_if_new_day(runtime: &mut RuntimeState) {
    if runtime.stats.date != today_string() {
        runtime
            .history
            .insert(runtime.stats.date.clone(), runtime.stats.clone());
        runtime.stats = fresh_stats();
        runtime
            .history
            .insert(runtime.stats.date.clone(), runtime.stats.clone());
        runtime.recent_events.clear();
    }
}

fn process_key_press(state: &KeyPulseState, app: &AppHandle, key_name: String) {
    let Ok(mut runtime) = state.runtime.lock() else {
        return;
    };
    reset_if_new_day(&mut runtime);

    if is_modifier(&key_name) {
        update_modifier(&mut runtime.modifiers, &key_name, true);
    }

    let category = category_for_key(&key_name);
    let now = now_ms();
    runtime.recent_events.push_back(now);
    while runtime
        .recent_events
        .front()
        .is_some_and(|first| now.saturating_sub(*first) > 60_000)
    {
        runtime.recent_events.pop_front();
    }

    runtime.stats.total_keys += 1;
    runtime.stats.current_minute_keys = runtime.recent_events.len() as u64;
    runtime.stats.peak_per_minute = runtime
        .stats
        .peak_per_minute
        .max(runtime.stats.current_minute_keys);
    runtime.stats.updated_at = now_string();
    let timestamp = Local::now();
    let hour = timestamp.hour() as usize;
    let half_hour = hour * 2 + usize::from(timestamp.minute() >= 30);
    runtime.stats.half_hourly_counts.resize(48, 0);
    runtime.stats.hourly_counts[hour] += 1;
    runtime.stats.half_hourly_counts[half_hour] += 1;
    bump(&mut runtime.stats.category_counts, "ordinary");
    bump(&mut runtime.stats.category_counts, category);

    if let Some(shortcut) = shortcut_label(&runtime.modifiers, &key_name) {
        bump(&mut runtime.stats.category_counts, "shortcut");
        bump(&mut runtime.stats.shortcut_counts, &shortcut);
    }

    if now.saturating_sub(runtime.last_save_ms) > 900 {
        persist_runtime(state, &mut runtime);
        runtime.last_save_ms = now;
    }

    let snapshot = snapshot_from_runtime(
        &runtime,
        state.running.load(Ordering::SeqCst),
        &state.storage_path,
    );
    let _ = app.emit(STATS_EVENT, snapshot);
}

fn process_key_release(state: &KeyPulseState, key_name: String) {
    if !is_modifier(&key_name) {
        return;
    }
    if let Ok(mut runtime) = state.runtime.lock() {
        update_modifier(&mut runtime.modifiers, &key_name, false);
    }
}

fn process_modifier_change(state: &KeyPulseState, app: &AppHandle, key_name: String, down: bool) {
    let was_down = state
        .runtime
        .lock()
        .map(|runtime| modifier_is_active(&runtime.modifiers, &key_name))
        .unwrap_or(false);
    if down && !was_down {
        if state.running.load(Ordering::SeqCst) {
            process_key_press(state, app, key_name);
        } else if let Ok(mut runtime) = state.runtime.lock() {
            update_modifier(&mut runtime.modifiers, &key_name, true);
        }
    } else if !down && was_down {
        process_key_release(state, key_name);
    }
}

#[cfg(not(target_os = "macos"))]
fn listener_event(state: Arc<KeyPulseState>, app: AppHandle, event: Event) {
    match event.event_type {
        EventType::KeyPress(key) => {
            if !state.running.load(Ordering::SeqCst) {
                return;
            }
            process_key_press(&state, &app, format!("{key:?}"));
        }
        EventType::KeyRelease(key) => {
            process_key_release(&state, format!("{key:?}"));
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
fn mac_listener_event(
    state: &Arc<KeyPulseState>,
    app: &AppHandle,
    event_type: CGEventType,
    event: &CGEvent,
) {
    let key_name =
        mac_key_name(event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16);
    match event_type {
        CGEventType::KeyDown => {
            if state.running.load(Ordering::SeqCst) {
                process_key_press(state, app, key_name.to_string());
            }
        }
        CGEventType::KeyUp => {
            process_key_release(state, key_name.to_string());
        }
        CGEventType::FlagsChanged => {
            if is_modifier(key_name) {
                process_modifier_change(
                    state,
                    app,
                    key_name.to_string(),
                    mac_modifier_is_down(event.get_flags(), key_name),
                );
            }
        }
        _ => {}
    }
}

#[cfg(target_os = "macos")]
fn listen_macos(
    state: Arc<KeyPulseState>,
    app: AppHandle,
    mut ready: Option<mpsc::Sender<Result<(), String>>>,
) -> Result<(), String> {
    CGEventTap::with_enabled(
        CGEventTapLocation::Session,
        CGEventTapPlacement::HeadInsertEventTap,
        CGEventTapOptions::ListenOnly,
        vec![
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ],
        move |_proxy, event_type, event| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                mac_listener_event(&state, &app, event_type, event);
            }));
            CallbackResult::Keep
        },
        || {
            if let Some(sender) = ready.take() {
                let _ = sender.send(Ok(()));
            }
            CFRunLoop::run_current();
        },
    )
    .map_err(|_| {
        "macOS event tap 创建失败，请确认已授予 Input Monitoring 或 Accessibility 权限。"
            .to_string()
    })
}

fn ensure_listener(state: Arc<KeyPulseState>, app: AppHandle) -> Result<(), String> {
    if state.listener_started.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        if !mac_listen_event_access_granted() {
            state.listener_started.store(false, Ordering::SeqCst);
            return Err("请在 macOS 系统设置 > 隐私与安全性 > 输入监控 中允许 KeyPulse。若已允许，请重启 KeyPulse 让权限生效。".to_string());
        }
    }
    #[cfg(target_os = "macos")]
    let (ready_tx, ready_rx) = mpsc::channel();
    let listener_state_for_error = state.clone();
    let app_for_error = app.clone();
    thread::spawn(move || {
        #[cfg(target_os = "macos")]
        {
            let listener_state = state.clone();
            if let Err(error) = listen_macos(listener_state.clone(), app.clone(), Some(ready_tx)) {
                listener_state.running.store(false, Ordering::SeqCst);
                listener_state
                    .listener_started
                    .store(false, Ordering::SeqCst);
                eprintln!("{error}");
                let _ = app.emit("listener-error", error);
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let listener_state = state.clone();
            let callback_state = listener_state.clone();
            let callback_app = app.clone();
            if let Err(error) = listen(move |event| {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    listener_event(callback_state.clone(), callback_app.clone(), event);
                }));
            }) {
                listener_state.running.store(false, Ordering::SeqCst);
                listener_state
                    .listener_started
                    .store(false, Ordering::SeqCst);
                eprintln!("{error:?}");
                let _ = app.emit("listener-error", format!("{error:?}"));
            }
        }
    });
    #[cfg(target_os = "macos")]
    {
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                listener_state_for_error
                    .listener_started
                    .store(false, Ordering::SeqCst);
                Err(error)
            }
            Err(_) => {
                listener_state_for_error
                    .listener_started
                    .store(false, Ordering::SeqCst);
                let error =
                    "监听器启动超时，请确认 KeyPulse 已获得输入监控权限后重试。".to_string();
                let _ = app_for_error.emit("listener-error", error.clone());
                Err(error)
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

fn permission_hint() -> String {
    if cfg!(target_os = "macos") {
        "macOS 需要在 系统设置 > 隐私与安全性 > 输入监控 中允许 KeyPulse；授权后请重启应用，重新安装后可能需要关闭再打开一次授权。".to_string()
    } else if cfg!(target_os = "windows") {
        "Windows 首次运行可能需要安全软件允许 KeyPulse 监听全局键盘事件。".to_string()
    } else {
        "当前平台可能需要额外的系统权限。".to_string()
    }
}

#[tauri::command]
fn get_snapshot(state: State<'_, Arc<KeyPulseState>>) -> StatsSnapshot {
    let runtime = state.runtime.lock().unwrap();
    snapshot_from_runtime(
        &runtime,
        state.running.load(Ordering::SeqCst),
        &state.storage_path,
    )
}

#[tauri::command]
fn start_listening(
    app: AppHandle,
    state: State<'_, Arc<KeyPulseState>>,
) -> Result<StatsSnapshot, String> {
    state.running.store(true, Ordering::SeqCst);
    if let Err(error) = ensure_listener(state.inner().clone(), app.clone()) {
        state.running.store(false, Ordering::SeqCst);
        let _ = app.emit("listener-error", error.clone());
        let _ = open_permissions();
        return Err(error);
    }
    Ok(get_snapshot(state))
}

#[tauri::command]
fn stop_listening(state: State<'_, Arc<KeyPulseState>>) -> StatsSnapshot {
    state.running.store(false, Ordering::SeqCst);
    let mut runtime = state.runtime.lock().unwrap();
    runtime.modifiers = Modifiers::default();
    persist_runtime(&state, &mut runtime);
    snapshot_from_runtime(&runtime, false, &state.storage_path)
}

#[tauri::command]
fn reset_today(state: State<'_, Arc<KeyPulseState>>) -> StatsSnapshot {
    let mut runtime = state.runtime.lock().unwrap();
    runtime.stats = fresh_stats();
    runtime.recent_events.clear();
    runtime.modifiers = Modifiers::default();
    persist_runtime(&state, &mut runtime);
    snapshot_from_runtime(
        &runtime,
        state.running.load(Ordering::SeqCst),
        &state.storage_path,
    )
}

#[tauri::command]
fn open_permissions() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            .spawn()
            .map_err(|error| error.to_string())?;
    } else if cfg!(target_os = "windows") {
        Command::new("cmd")
            .args(["/C", "start", "ms-settings:privacy"])
            .spawn()
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn repair_permissions() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let _ = Command::new("tccutil")
            .args(["reset", "ListenEvent", "cn.xingshi.keypulse"])
            .status();
        let _ = Command::new("tccutil")
            .args(["reset", "Accessibility", "cn.xingshi.keypulse"])
            .status();
        open_permissions()?;
    }
    Ok(())
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    app.request_restart();
}

#[cfg(target_os = "macos")]
fn enter_menu_bar_mode(app: &AppHandle) {
    let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    let _ = app.set_dock_visibility(false);
}

fn show_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
        let _ = app.show();
        let _ = app.set_dock_visibility(true);
    }
    let window = app
        .get_webview_window("main")
        .or_else(|| app.webview_windows().into_values().next())
        .or_else(|| {
            match WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("KeyPulse")
                .inner_size(980.0, 680.0)
                .min_inner_size(760.0, 540.0)
                .resizable(true)
                .center()
                .build()
            {
                Ok(window) => Some(window),
                Err(error) => {
                    eprintln!("failed to create KeyPulse main window: {error}");
                    None
                }
            }
        });
    if let Some(window) = window {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        #[cfg(target_os = "macos")]
        {
            let _ = window.set_always_on_top(true);
            let _ = window.set_always_on_top(false);
        }
    }
}

fn build_state() -> Arc<KeyPulseState> {
    let storage_path = stats_path();
    let history_path = history_path();
    let stats = load_stats(&storage_path);
    let mut history = load_history(&history_path);
    history.insert(stats.date.clone(), stats.clone());
    Arc::new(KeyPulseState {
        runtime: Arc::new(Mutex::new(RuntimeState {
            stats,
            history,
            recent_events: VecDeque::new(),
            modifiers: Modifiers::default(),
            last_save_ms: 0,
        })),
        running: Arc::new(AtomicBool::new(false)),
        listener_started: AtomicBool::new(false),
        tray_icon: Mutex::new(None),
        storage_path,
        history_path,
    })
}

pub fn run() {
    let app = tauri::Builder::default()
        .manage(build_state())
        .setup(|app| {
            let show_item = MenuItem::with_id(app, "show", "显示 KeyPulse", true, None::<&str>)?;
            let start_item = MenuItem::with_id(app, "start", "开始监听", true, None::<&str>)?;
            let stop_item = MenuItem::with_id(app, "stop", "暂停监听", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_item, &start_item, &stop_item, &quit_item])?;
            let tray_builder = TrayIconBuilder::with_id("keypulse")
                .menu(&menu)
                .tooltip("KeyPulse 正在后台运行")
                .show_menu_on_left_click(false);
            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder.title("键");
            #[cfg(not(target_os = "macos"))]
            let tray_builder = if let Some(icon) = app.default_window_icon().cloned() {
                tray_builder.icon(icon)
            } else {
                tray_builder
            };
            let _tray = tray_builder
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "start" => {
                        if let Some(state) = app.try_state::<Arc<KeyPulseState>>() {
                            state.running.store(true, Ordering::SeqCst);
                            if let Err(error) = ensure_listener(state.inner().clone(), app.clone())
                            {
                                state.running.store(false, Ordering::SeqCst);
                                let _ = app.emit("listener-error", error);
                                let _ = open_permissions();
                            }
                        }
                        show_main_window(app);
                    }
                    "stop" => {
                        if let Some(state) = app.try_state::<Arc<KeyPulseState>>() {
                            state.running.store(false, Ordering::SeqCst);
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(&tray.app_handle());
                    }
                })
                .build(app)?;
            let _ = _tray.set_visible(true);
            if let Some(state) = app.try_state::<Arc<KeyPulseState>>() {
                *state.tray_icon.lock().unwrap() = Some(_tray);
            }
            show_main_window(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                {
                    let app = window.app_handle();
                    enter_menu_bar_mode(&app);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            start_listening,
            stop_listening,
            reset_today,
            open_permissions,
            repair_permissions,
            restart_app
        ])
        .build(tauri::generate_context!())
        .expect("error while building KeyPulse");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::Ready => show_main_window(app_handle),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => show_main_window(app_handle),
        _ => {}
    });
}
