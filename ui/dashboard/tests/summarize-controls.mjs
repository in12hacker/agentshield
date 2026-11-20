// Actual DOM observations by project and owner. Operations do not imply result assertions.
import { readdir, readFile, writeFile } from 'node:fs/promises';
import { resolve, join, relative } from 'node:path';

const root = resolve(process.argv[2] ?? 'test-results');
const controls = new Map();
async function visit(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) await visit(path);
    else if (entry.name === 'dom-controls.json') {
      const report = JSON.parse(await readFile(path, 'utf8'));
      if (typeof report.project !== 'string' || !Array.isArray(report.controls)) throw Error(`Not a current DOM inventory: ${path}`);
      for (const control of report.controls) for (const observation of control.states) {
        if (typeof observation.path !== 'string') throw Error(`Missing owner-relative DOM path: ${path}`);
        const { page, module, scope, path: domPath, region, fieldsets, context, category, group, tag, id, type, name, href, label } = observation;
        const shared = module === 'global' || module === 'ask' || module === 'gate';
        // Fixed shell/gate/overview controls retain their physical identity across language/status text changes.
        // Business controls keep their field/section context and label; conditional sibling positions can move.
        const fixed = module === 'global' || module === 'gate' || module === 'overview';
        const identity = { project: report.project, page: shared ? module : page, module, scope,
          ...(fixed ? { path: domPath } : { label }), region,
          fieldsets, context, category, group, tag, id, type, name, href };
        const key = JSON.stringify(identity);
        const item = controls.get(key) ?? { ...identity, labels: [], status: 'NOT_RUN', observations: [] };
        if (!item.labels.includes(label)) item.labels.push(label);
        item.observations.push({ file: relative(root, path), test: report.test, testResult: report.testResult, page, label,
          path: domPath, actions: observation.actions, focused: observation.focused, hidden: observation.hidden,
          disabled: observation.disabled, readOnly: observation.readOnly, reasons: observation.reasons });
        if (observation.actions.length) item.status = 'OPERATED';
        controls.set(key, item);
      }
    }
  }
}
for (const directory of process.argv.length > 2 ? process.argv.slice(2).map(path => resolve(path)) : [root]) await visit(directory);
for (const control of controls.values()) {
  if (control.status !== 'OPERATED' && control.observations.every(item => item.disabled || item.hidden || item.readOnly)) control.status = 'blocked';
}
const projects = {};
for (const control of controls.values()) {
  const pages = projects[control.project] ??= {};
  (pages[control.page] ??= []).push(control);
}
const result = { meaning: 'DOM input/activation observations only, grouped by project and actual owner; read the linked spec assertions and interface events for results.', projects };
await writeFile(join(root, 'control-inventory.json'), JSON.stringify(result, null, 2));
for (const item of controls.values()) if (item.status === 'NOT_RUN') {
  console.log(JSON.stringify({ project: item.project, page: item.page, scope: item.scope, status: item.status,
    tag: item.tag, labels: item.labels, fieldsets: item.fieldsets, context: item.context, region: item.region, path: item.path }));
}
console.log(`DOM evidence: ${join(root, 'control-inventory.json')}`);
