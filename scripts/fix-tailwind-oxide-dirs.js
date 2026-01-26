#!/usr/bin/env node

/**
 * This script creates empty placeholder directories for Tailwind CSS oxide
 * platform-specific packages that don't exist but electron-builder tries to scan.
 * This prevents ENOENT errors during the electron-builder packaging phase.
 */

const fs = require('fs');
const path = require('path');

const nodeModulesPath = path.join(__dirname, '..', 'node_modules', '@tailwindcss');
const oxideDirs = [
  'oxide-android-arm64',
  'oxide-android-arm64-gnu',
  'oxide-android-arm64-musl',
  'oxide-android-x64',
  'oxide-android-x64-gnu',
  'oxide-android-x64-musl',
  'oxide-darwin-arm64',
  'oxide-darwin-x64',
  'oxide-freebsd-arm64',
  'oxide-freebsd-x64',
  'oxide-linux-arm64-gnu',
  'oxide-linux-arm64-musl',
  'oxide-linux-arm-gnueabihf',
  'oxide-linux-riscv64-gnu',
  'oxide-linux-s390x-gnu',
  'oxide-netbsd-x64',
  'oxide-openbsd-x64',
  'oxide-win32-arm64-msvc',
  'oxide-win32-ia32-msvc',
  'oxide-win32-x64-msvc',
];

// Ensure @tailwindcss directory exists
if (!fs.existsSync(nodeModulesPath)) {
  console.log('@tailwindcss directory does not exist, skipping...');
  process.exit(0);
}

let created = 0;
let skipped = 0;

oxideDirs.forEach(dir => {
  const dirPath = path.join(nodeModulesPath, dir);
  if (!fs.existsSync(dirPath)) {
    try {
      fs.mkdirSync(dirPath, { recursive: true });
      // Create a minimal package.json to make it a valid package directory
      const packageJsonPath = path.join(dirPath, 'package.json');
      if (!fs.existsSync(packageJsonPath)) {
        fs.writeFileSync(packageJsonPath, JSON.stringify({
          name: `@tailwindcss/${dir}`,
          version: '0.0.0',
          description: 'Placeholder package',
        }, null, 2));
      }
      created++;
    } catch (error) {
      console.error(`Failed to create ${dirPath}:`, error.message);
    }
  } else {
    skipped++;
  }
});

if (created > 0) {
  console.log(`Created ${created} placeholder directories for missing Tailwind CSS oxide packages.`);
}
if (skipped > 0) {
  console.log(`Skipped ${skipped} directories that already exist.`);
}
