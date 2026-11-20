import { readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import ts from 'typescript';
import { locales, messagePlaceholder } from '../src/i18n-contract.ts';

const dashboard = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const copyAttributes = new Set(['alt', 'title', 'placeholder', 'label', 'aria-label',
  'aria-description', 'aria-roledescription', 'aria-valuetext']);
const idAttributes = new Set(['className', 'aria-describedby', 'aria-labelledby',
  'aria-controls', 'aria-owns', 'aria-details']);
const visibleProperty = /^(label|title|description|placeholder|message)$/i;
const languageText = /[\p{L}\p{N}]/u;
const cjk = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}]/u;
const phrase = /[A-Za-z][ \t]+[A-Za-z]/;

function isLiteral(node) {
  return ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node);
}

function readCatalog(file) {
  const source = ts.parseJsonText(file, readFileSync(file, 'utf8'));
  const entry = source.statements[0];
  if (source.parseDiagnostics.length || source.statements.length !== 1
    || !ts.isExpressionStatement(entry) || !ts.isObjectLiteralExpression(entry.expression)) {
    throw Error(`${file}: expected one JSON object`);
  }
  const catalog = new Map();
  for (const property of entry.expression.properties) {
    if (!ts.isPropertyAssignment(property) || !ts.isStringLiteral(property.name)
      || !ts.isStringLiteral(property.initializer) || !property.initializer.text.trim()) {
      throw Error(`${file}: expected nonempty translation strings`);
    }
    const key = property.name.text;
    if (catalog.has(key)) throw Error(`${file}: duplicate translation key ${key}`);
    catalog.set(key, property.initializer.text);
  }
  if (!catalog.size) throw Error(`${file}: empty translation catalog`);
  return catalog;
}

function placeholders(text) {
  return [...new Set(Array.from(text.matchAll(messagePlaceholder), match => match[1]))].sort().join(',');
}

function* sources(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) yield* sources(path);
    else if (/\.tsx?$/.test(entry.name) && !/\.(?:test|spec|d)\.tsx?$/.test(entry.name)) yield path;
  }
}

function hasVisibleLiteral(node) {
  if (!node) return false;
  if (isLiteral(node)) return languageText.test(node.text);
  if (ts.isTemplateExpression(node)) {
    return languageText.test(node.head.text) || node.templateSpans.some(span => languageText.test(span.literal.text));
  }
  if (ts.isParenthesizedExpression(node)) return hasVisibleLiteral(node.expression);
  if (ts.isConditionalExpression(node)) return hasVisibleLiteral(node.whenTrue) || hasVisibleLiteral(node.whenFalse);
  if (ts.isBinaryExpression(node)) {
    if (node.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken) return hasVisibleLiteral(node.right);
    if ([ts.SyntaxKind.PlusToken, ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken]
      .includes(node.operatorToken.kind)) return hasVisibleLiteral(node.left) || hasVisibleLiteral(node.right);
  }
  return false;
}

function attributeName(node, source) {
  for (let parent = node.parent; parent && !ts.isSourceFile(parent); parent = parent.parent) {
    if (ts.isJsxAttribute(parent)) return parent.name.getText(source);
  }
  return '';
}

function inspect(root, file, keys, failures) {
  const source = ts.createSourceFile(file, readFileSync(file, 'utf8'), ts.ScriptTarget.Latest, true);
  const report = (node, problem) => {
    const position = source.getLineAndCharacterOfPosition(node.getStart(source));
    failures.add(`${relative(root, file)}:${position.line + 1}:${position.character + 1}: ${problem}`);
  };
  function visit(node) {
    if (ts.isJsxText(node) && languageText.test(node.text)) report(node, 'literal UI text');
    if (ts.isJsxAttribute(node) && copyAttributes.has(node.name.getText(source))) {
      const initializer = node.initializer;
      if (hasVisibleLiteral(initializer && ts.isJsxExpression(initializer) ? initializer.expression : initializer)) {
        report(node, 'literal visible attribute');
      }
    }
    if (ts.isJsxExpression(node) && !ts.isJsxAttribute(node.parent)
      && hasVisibleLiteral(node.expression)) report(node, 'literal UI expression');
    if (isLiteral(node) || ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node)) {
      if (cjk.test(node.text) || (phrase.test(node.text) && !idAttributes.has(attributeName(node, source)))) {
        report(node, 'literal product copy outside a locale catalog');
      }
      if ((ts.isPropertyAssignment(node.parent) || ts.isVariableDeclaration(node.parent))
        && node.parent.initializer === node && visibleProperty.test(node.parent.name.getText(source))
        && /^[A-Z][A-Za-z]+$/.test(node.text)) report(node, 'literal product label');
    }
    if (ts.isCallExpression(node) && ts.isIdentifier(node.expression)) {
      const key = node.expression.text === 't' ? node.arguments[0]
        : node.expression.text === 'translate' ? node.arguments[1] : undefined;
      if (key && isLiteral(key) && !keys.has(key.text)) report(key, `unknown translation key ${key.text}`);
    }
    ts.forEachChild(node, visit);
  }
  visit(source);
}

export function checkI18n(root = dashboard) {
  const directory = join(root, 'src', 'locales');
  const catalogs = locales.map(locale => [locale, readCatalog(join(directory, `${locale}.json`))]);
  const keys = new Set(catalogs.flatMap(([, catalog]) => [...catalog.keys()]));
  const failures = new Set();
  for (const key of keys) {
    const text = catalogs.find(([, catalog]) => catalog.has(key))[1].get(key);
    const expected = placeholders(text);
    for (const [locale, catalog] of catalogs) {
      const translation = catalog.get(key);
      if (translation === undefined) failures.add(`${locale}: missing ${key}`);
      else if (expected !== placeholders(translation)) failures.add(`${locale}: parameters differ for ${key}`);
    }
  }
  for (const file of sources(join(root, 'src'))) inspect(root, file, keys, failures);
  if (failures.size) throw Error(`i18n check failed\n${[...failures].join('\n')}`);
  return keys.size;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    const count = checkI18n();
    console.log(`i18n OK: ${locales.join(', ')} · ${count} keys`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
