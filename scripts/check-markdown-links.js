#!/usr/bin/env node
/**
 * Local Markdown link checker for repository documentation (Issue #353).
 */
const fs = require('fs');
const path = require('path');

function getMarkdownFiles(dir, fileList = []) {
  const files = fs.readdirSync(dir);
  for (const file of files) {
    if (file === 'node_modules' || file === '.git' || file === 'target') continue;
    const fullPath = path.join(dir, file);
    if (fs.statSync(fullPath).isDirectory()) {
      getMarkdownFiles(fullPath, fileList);
    } else if (file.endsWith('.md')) {
      fileList.push(fullPath);
    }
  }
  return fileList;
}

const docsDir = path.resolve(__dirname, '..', 'docs');
const files = getMarkdownFiles(docsDir);
let hasBrokenLinks = false;

console.log(`Checking Markdown links across ${files.length} documents...`);

for (const file of files) {
  const content = fs.readFileSync(file, 'utf8');
  const linkRegex = /\[([^\]]+)\]\(([^)]+)\)/g;
  let match;
  while ((match = linkRegex.exec(content)) !== null) {
    const target = match[2];
    if (target.startsWith('http://') || target.startsWith('https://') || target.startsWith('#')) {
      continue;
    }
    const cleanTarget = target.split('#')[0];
    if (!cleanTarget) continue;
    const resolvedPath = path.resolve(path.dirname(file), cleanTarget);
    if (!fs.existsSync(resolvedPath)) {
      console.warn(`[BROKEN LINK] in ${path.relative(process.cwd(), file)}: target not found -> ${target}`);
      hasBrokenLinks = true;
    }
  }
}

if (hasBrokenLinks) {
  console.warn('Completed with link warnings.');
} else {
  console.log('All local markdown links verified successfully!');
}
