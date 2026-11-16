import fs from 'node:fs';
const expressionPath = process.argv[2];
if (!expressionPath) throw new Error('missing PAGE_FORMS_JS path');
const expression = fs.readFileSync(expressionPath, 'utf8');

function field(name, id) {
  return {
    nodeType: 1,
    tagName: 'INPUT',
    id,
    name,
    type: 'text',
    required: false,
    disabled: false,
    readOnly: false,
    value: '',
    labels: [],
    parentElement: null,
    getAttribute() { return null; },
  };
}

const fields = [
  field('name', 'field-name'),
  field('action', 'field-action'),
  field('method', 'field-method'),
];

const form = {
  nodeType: 1,
  tagName: 'FORM',
  parentElement: null,
  children: [],
  querySelectorAll(selector) {
    if (selector === 'input, select, textarea, button') return fields;
    return [];
  },
  getAttribute(attribute) {
    if (attribute === 'name') return 'named-controls-form';
    if (attribute === 'id') return 'f1';
    if (attribute === 'action') return '/submit?named=1';
    if (attribute === 'method') return 'POST';
    return null;
  },
  // A real HTMLFormElement exposes controls through named-property access.
  // These getters intentionally return objects to catch any regression back
  // to form.name / form.id / form.action / form.method.
  get name() { return fields[0]; },
  get id() { return { value: 'f1' }; },
  get action() { return { value: '/submit?named=1' }; },
  get method() { return { value: 'POST' }; },
};

const blankForm = {
  nodeType: 1,
  tagName: 'FORM',
  parentElement: null,
  children: [],
  querySelectorAll() { return []; },
  getAttribute() { return null; },
};

globalThis.document = {
  querySelectorAll(selector) {
    if (selector === '*') return [];
    if (selector === 'form') return [form, blankForm];
    return [];
  },
  querySelector() { return null; },
};
globalThis.location = { href: 'http://127.0.0.1:8821/page.html' };
globalThis.CSS = { escape: (value) => String(value) };

const result = eval(expression);
if (!result || !Array.isArray(result.forms) || result.forms.length !== 2) {
  throw new Error('expected two forms: ' + JSON.stringify(result));
}
const [named, blank] = result.forms;
for (const key of ['name', 'id', 'action', 'method']) {
  if (typeof named[key] !== 'string') {
    throw new Error(key + ' must be a string: ' + JSON.stringify(named[key]));
  }
}
if (named.name !== 'named-controls-form') throw new Error('name: ' + named.name);
if (named.id !== 'f1') throw new Error('id: ' + named.id);
if (named.action !== 'http://127.0.0.1:8821/submit?named=1') throw new Error('action: ' + named.action);
if (named.method !== 'post') throw new Error('method: ' + named.method);
if (!Array.isArray(named.fields) || named.fields.length !== 3) throw new Error('fields');
if (blank.action !== 'http://127.0.0.1:8821/page.html') throw new Error('blank action: ' + blank.action);
if (blank.method !== 'get') throw new Error('blank method: ' + blank.method);
if ('name' in blank || 'id' in blank) throw new Error('blank form must omit name/id');
console.log('page_forms named controls: ok');
