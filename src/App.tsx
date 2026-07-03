import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  Activity,
  AlertTriangle,
  BarChart3,
  CheckCircle2,
  Clock3,
  Keyboard,
  LockKeyhole,
  Pause,
  Play,
  RefreshCcw,
  ShieldCheck,
} from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';

interface KeyStats {
  date: string;
  startedAt: string;
  updatedAt: string;
  totalKeys: number;
  currentMinuteKeys: number;
  peakPerMinute: number;
  categoryCounts: Record<string, number>;
  shortcutCounts: Record<string, number>;
  hourlyCounts: number[];
}

interface CountItem {
  count: number;
}

interface ShortcutCount extends CountItem {
  shortcut: string;
}

interface CategoryCount extends CountItem {
  category: string;
}

interface StatsSnapshot {
  listening: boolean;
  inputMonitoringGranted: boolean;
  permissionHint: string;
  storagePath: string;
  stats: KeyStats;
  topShortcuts: ShortcutCount[];
  topCategories: CategoryCount[];
}

const categoryLabels: Record<string, string> = {
  ordinary: '总按键',
  letter: '字母类',
  number: '数字类',
  enter: 'Enter',
  backspace: 'Backspace',
  tab: 'Tab',
  escape: 'Esc',
  arrow: '方向键',
  function: '功能键',
  modifier: '修饰键',
  shortcut: '快捷键',
  other: '其他',
};

function compactNumber(value: number): string {
  return new Intl.NumberFormat('zh-CN').format(value || 0);
}

function formatTime(value: string): string {
  if (!value) return '--';
  return new Date(value).toLocaleTimeString('zh-CN', {
    hour12: false,
    hour: '2-digit',
    minute: '2-digit',
  });
}

function emptySnapshot(): StatsSnapshot {
  return {
    listening: false,
    inputMonitoringGranted: false,
    permissionHint: '',
    storagePath: '',
    stats: {
      date: '',
      startedAt: '',
      updatedAt: '',
      totalKeys: 0,
      currentMinuteKeys: 0,
      peakPerMinute: 0,
      categoryCounts: {},
      shortcutCounts: {},
      hourlyCounts: Array.from({ length: 24 }, () => 0),
    },
    topShortcuts: [],
    topCategories: [],
  };
}

function App() {
  const [snapshot, setSnapshot] = useState<StatsSnapshot>(emptySnapshot);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState('');

  const stats = snapshot.stats;
  const maxHour = Math.max(1, ...stats.hourlyCounts);
  const categoryRows = useMemo(() => {
    return [
      'letter',
      'number',
      'enter',
      'backspace',
      'tab',
      'escape',
      'arrow',
      'function',
      'modifier',
      'shortcut',
      'other',
    ].map((key) => ({
      key,
      label: categoryLabels[key] || key,
      count: stats.categoryCounts[key] || 0,
    }));
  }, [stats.categoryCounts]);

  async function refresh() {
    const next = await invoke<StatsSnapshot>('get_snapshot');
    setSnapshot(next);
  }

  async function start() {
    setBusy(true);
    try {
      const next = await invoke<StatsSnapshot>('start_listening');
      setSnapshot(next);
      setNotice('监听已开启');
    } catch (error) {
      setNotice(error instanceof Error ? error.message : String(error));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    try {
      const next = await invoke<StatsSnapshot>('stop_listening');
      setSnapshot(next);
      setNotice('监听已暂停');
    } finally {
      setBusy(false);
    }
  }

  async function reset() {
    if (!window.confirm('清空今天的聚合统计？')) return;
    const next = await invoke<StatsSnapshot>('reset_today');
    setSnapshot(next);
    setNotice('今日统计已清空');
  }

  async function openPermissions() {
    await invoke('open_permissions');
    setNotice('已打开系统权限设置，授权后请重启 KeyPulse 生效');
    window.setTimeout(() => void refresh(), 1200);
  }

  async function restartApp() {
    setNotice('正在重启 KeyPulse');
    await invoke('restart_app');
  }

  useEffect(() => {
    void refresh();
    const unlistenStats = listen<StatsSnapshot>('stats-updated', (event) => {
      setSnapshot(event.payload);
    });
    const unlistenError = listen<string>('listener-error', (event) => {
      setNotice(event.payload);
      void refresh();
    });
    return () => {
      void unlistenStats.then((unlisten) => unlisten());
      void unlistenError.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    if (!notice) return undefined;
    const timer = window.setTimeout(() => setNotice(''), 2800);
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      void refresh();
    }, snapshot.inputMonitoringGranted ? 12000 : 2500);
    const onFocus = () => {
      void refresh();
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('focus', onFocus);
    };
  }, [snapshot.inputMonitoringGranted]);

  return (
    <main className="app-shell">
      <header className="titlebar">
        <div>
          <span className="eyebrow">KeyPulse</span>
          <h1>键盘频率仪</h1>
        </div>
        <div className={`live-pill ${snapshot.listening ? 'is-live' : ''}`}>
          <span />
          {snapshot.listening ? '监听中' : '已暂停'}
        </div>
      </header>

      <section className="privacy-strip">
        <ShieldCheck size={18} />
        <div>
          <strong>隐私安全模式</strong>
          <small>只保存聚合统计。普通字母和数字不会以具体字符或输入顺序落盘。</small>
        </div>
        <div className={`permission-badge ${snapshot.inputMonitoringGranted ? 'is-ok' : 'is-warn'}`}>
          {snapshot.inputMonitoringGranted ? <CheckCircle2 size={15} /> : <AlertTriangle size={15} />}
          {snapshot.inputMonitoringGranted ? '输入监控已生效' : '输入监控待生效'}
        </div>
        <div className="permission-actions">
          <button type="button" className="ghost-button" onClick={openPermissions}>
            <LockKeyhole size={16} />
            权限
          </button>
          {!snapshot.inputMonitoringGranted ? (
            <button type="button" className="ghost-button" onClick={restartApp}>
              <RefreshCcw size={16} />
              重启生效
            </button>
          ) : null}
        </div>
      </section>

      <section className="hero-grid">
        <div className="metric-tile primary">
          <Keyboard size={24} />
          <span>最近一分钟</span>
          <strong>{compactNumber(stats.currentMinuteKeys)}</strong>
          <small>keys/min</small>
        </div>
        <div className="metric-tile">
          <Activity size={22} />
          <span>今日总量</span>
          <strong>{compactNumber(stats.totalKeys)}</strong>
          <small>次敲击</small>
        </div>
        <div className="metric-tile">
          <BarChart3 size={22} />
          <span>分钟峰值</span>
          <strong>{compactNumber(stats.peakPerMinute)}</strong>
          <small>keys/min</small>
        </div>
        <div className="metric-tile">
          <Clock3 size={22} />
          <span>最近更新</span>
          <strong>{formatTime(stats.updatedAt)}</strong>
          <small>{stats.date || '等待数据'}</small>
        </div>
      </section>

      <section className="control-bar">
        {snapshot.listening ? (
          <button type="button" className="button secondary" onClick={stop} disabled={busy}>
            <Pause size={17} />
            暂停
          </button>
        ) : (
          <button type="button" className="button primary" onClick={start} disabled={busy}>
            <Play size={17} />
            开始监听
          </button>
        )}
        <button type="button" className="button secondary" onClick={refresh} disabled={busy}>
          <RefreshCcw size={17} />
          刷新
        </button>
        <button type="button" className="button quiet" onClick={reset} disabled={busy || stats.totalKeys === 0}>
          清空今日
        </button>
      </section>

      <section className="content-grid">
        <div className="panel">
          <div className="panel-header">
            <strong>按键类别</strong>
            <small>普通字符只保留类别</small>
          </div>
          <div className="category-list">
            {categoryRows.map((item) => (
              <div className="category-row" key={item.key}>
                <span>{item.label}</span>
                <div>
                  <i style={{ width: `${Math.min(100, (item.count / Math.max(1, stats.totalKeys)) * 100)}%` }} />
                </div>
                <em>{compactNumber(item.count)}</em>
              </div>
            ))}
          </div>
        </div>

        <div className="panel">
          <div className="panel-header">
            <strong>快捷键排行</strong>
            <small>只统计带 Cmd/Ctrl/Alt 的组合</small>
          </div>
          <div className="shortcut-list">
            {snapshot.topShortcuts.length === 0 ? (
              <p className="empty">暂无快捷键数据</p>
            ) : snapshot.topShortcuts.map((item) => (
              <div className="shortcut-row" key={item.shortcut}>
                <kbd>{item.shortcut}</kbd>
                <span>{compactNumber(item.count)}</span>
              </div>
            ))}
          </div>
        </div>
      </section>

      <section className="panel heatmap-panel">
        <div className="panel-header">
          <strong>今日节奏</strong>
          <small>按小时聚合</small>
        </div>
        <div className="hour-grid">
          {stats.hourlyCounts.map((count, hour) => (
            <div className="hour-cell" key={hour}>
              <span style={{ height: `${Math.max(3, (count / maxHour) * 100)}%` }} />
              <small>{hour}</small>
            </div>
          ))}
        </div>
      </section>

      <footer className="footer">
        <span>{snapshot.permissionHint}</span>
        <code>{snapshot.storagePath}</code>
      </footer>

      {notice ? <div className="toast">{notice}</div> : null}
    </main>
  );
}

export default App;
