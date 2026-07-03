import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = dirname(dirname(fileURLToPath(import.meta.url)));

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: root,
    stdio: 'inherit',
    shell: false,
  });
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

if (process.platform === 'darwin') {
  run('bash', [join(root, 'scripts/ensure-macos-codesign-cert.sh')]);
  run('tauri', ['build', '--bundles', 'app']);
} else {
  run('tauri', ['build']);
}
