#!/usr/bin/env node
// Writes assets/regions.json: {locale: {ISO 3166-1 alpha-2: localized name}} for every UI locale,
// from the JS engine's CLDR data (`Intl.DisplayNames`, type "region"), as Fluxer's
// `getRegionDisplayName` resolves it at runtime. Names equal to en-US's are left out.
// Usage: node scripts/regions-extract.mjs
import {readdirSync, writeFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const locales = ['en-US', ...readdirSync(`${root}assets/i18n`).map((f) => f.replace(/\.json$/, '')).sort()];
const A = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const codes = [...A].flatMap((a) => [...A].map((b) => a + b));
const names = (loc) => {
	const d = new Intl.DisplayNames([loc], {type: 'region', fallback: 'none'});
	return Object.fromEntries(codes.map((c) => [c, d.of(c)]).filter(([, n]) => n));
};
const en = names('en-US');
const out = {'en-US': en};
for (const loc of locales.slice(1)) {
	out[loc] = Object.fromEntries(Object.entries(names(loc)).filter(([c, n]) => n !== en[c]));
}
writeFileSync(`${root}assets/regions.json`, `${JSON.stringify(out)}\n`);
