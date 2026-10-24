// The lint half of the gate (`just lint`): JavaScript, TypeScript and Astro components.
import { defineConfig, globalIgnores } from 'eslint/config';
import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import astro from 'eslint-plugin-astro';

export default defineConfig([
	globalIgnores(['dist/', '.astro/']),
	js.configs.recommended,
	tseslint.configs.recommended,
	astro.configs.recommended,
]);
