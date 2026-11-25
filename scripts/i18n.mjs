// Ferramentas de tradução do Larafim.
//
//   node scripts/i18n.mjs check   textos sem tradução (e traduções sem uso) por idioma
//   node scripts/i18n.mjs keys    todas as chaves encontradas no código (JSON)
//   node scripts/i18n.mjs sync    idiomas do instalador (tauri.conf.json + installer-hooks.nsh)
//   node scripts/i18n.mjs fill <código>   acrescenta ao JSON as chaves que faltam (valor vazio)
//
// A chave é o texto em português. Interface: t("…"), t('…') e plural(n, "…", "…").
// Rust: t!("…"), tr("…"), tf("…").
import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname, extname } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const localesDir = join(root, "locales");
const SOURCE = "pt-BR";

function walk(dir, exts, out = []) {
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "target" || name === "dist" || name.startsWith(".")) continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, exts, out);
    else if (exts.includes(extname(p))) out.push(p);
  }
  return out;
}

/** Literal JS/Rust ("…" ou '…', sem as aspas) → texto, numa passada só. */
function unquote(_q, body) {
  const simple = { n: "\n", r: "\r", t: "\t", 0: "\0", "\\": "\\", '"': '"', "'": "'" };
  return body.replace(/\\(u\{[0-9a-fA-F]{1,6}\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|.)/g, (_, e) => {
    if (e.startsWith("u{")) return String.fromCodePoint(parseInt(e.slice(2, -1), 16));
    if (e.length > 1) return String.fromCodePoint(parseInt(e.slice(1), 16));
    return simple[e] ?? e;
  });
}

function collectKeys() {
  const keys = new Map(); // chave → primeiro lugar onde aparece
  const add = (k, where) => {
    if (!keys.has(k)) keys.set(k, where);
  };
  const ui = walk(join(root, "app", "src"), [".svelte", ".ts"]).filter((f) => !f.endsWith("i18n.svelte.ts"));
  for (const f of ui) {
    const src = readFileSync(f, "utf8");
    // `\\.` e o resto nunca casam o mesmo caractere (sem `\` fora do escape):
    // assim a regex não volta atrás em cadeia (sem risco de ReDoS).
    for (const m of src.matchAll(/\bt\(\s*"((?:\\.|[^"\\\n])*)"/g)) add(unquote('"', m[1]), f);
    for (const m of src.matchAll(/\bt\(\s*'((?:\\.|[^'\\\n])*)'/g)) add(unquote("'", m[1]), f);
    for (const m of src.matchAll(/\bplural\([^,]+,\s*"((?:\\.|[^"\\])*)",\s*"((?:\\.|[^"\\])*)"/g)) {
      add(unquote('"', m[1]), f);
      add(unquote('"', m[2]), f);
    }
  }
  const rs = [...walk(join(root, "crates"), [".rs"]), ...walk(join(root, "app", "src-tauri", "src"), [".rs"])];
  for (const f of rs) {
    let src = readFileSync(f, "utf8");
    const cut = src.indexOf("#[cfg(test)]");
    if (cut >= 0) src = src.slice(0, cut);
    src = src.replace(/^\s*\/\/.*$/gm, ""); // comentários (exemplos de uso)
    for (const m of src.matchAll(/\b(?:t!|tr|tf)\(\s*"((?:\\.|[^"\\])*)"/g)) add(unquote('"', m[1]), f);
  }
  return keys;
}

function locales() {
  return readdirSync(localesDir)
    .filter((f) => f.endsWith(".json"))
    .map((f) => ({ code: f.replace(/\.json$/, ""), file: join(localesDir, f), data: JSON.parse(readFileSync(join(localesDir, f), "utf8")) }));
}

const cmd = process.argv[2] ?? "check";
const keys = collectKeys();

if (cmd === "keys") {
  console.log(JSON.stringify([...keys.keys()].sort(), null, 2));
} else if (cmd === "check") {
  let missingTotal = 0;
  for (const l of locales()) {
    if (l.code === SOURCE) continue;
    const missing = [...keys.keys()].filter((k) => typeof l.data[k] !== "string" || !l.data[k]);
    const unused = Object.keys(l.data).filter((k) => !k.startsWith("_") && !keys.has(k));
    const badParams = [...keys.keys()].filter((k) => {
      const v = l.data[k];
      if (typeof v !== "string" || !v) return false;
      const want = [...k.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
      const got = [...v.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
      return want !== got;
    });
    missingTotal += missing.length;
    console.log(`${l.code} (${l.data._meta?.name ?? ""}): ${keys.size - missing.length}/${keys.size} traduzidos`);
    for (const k of missing) console.log(`  falta: ${JSON.stringify(k)}  (${keys.get(k).replace(root, "")})`);
    for (const k of unused) console.log(`  sem uso: ${JSON.stringify(k)}`);
    for (const k of badParams) console.log(`  parâmetros diferentes: ${JSON.stringify(k)}`);
  }
  process.exitCode = missingTotal && process.argv.includes("--strict") ? 1 : 0;
} else if (cmd === "fill") {
  const code = process.argv[3];
  const l = locales().find((x) => x.code === code);
  if (!l) throw new Error(`idioma ${code} não encontrado em locales/`);
  for (const k of [...keys.keys()].sort()) if (!(k in l.data)) l.data[k] = "";
  writeFileSync(l.file, JSON.stringify(l.data, null, 2) + "\n");
  console.log(`${code}: chaves acrescentadas; preencha os valores vazios.`);
} else if (cmd === "sync") {
  // Instalador: um idioma NSIS por arquivo em locales/ (campo _meta.nsis / _meta.langid).
  const langs = locales().filter((l) => l.data._meta?.nsis && l.data._meta?.langid);
  langs.sort((a, b) => (a.code === SOURCE ? -1 : b.code === SOURCE ? 1 : a.code.localeCompare(b.code)));
  const confPath = join(root, "app", "src-tauri", "tauri.conf.json");
  const conf = JSON.parse(readFileSync(confPath, "utf8"));
  conf.bundle.windows.nsis.languages = langs.map((l) => l.data._meta.nsis);
  conf.bundle.windows.nsis.displayLanguageSelector = langs.length > 1;
  conf.bundle.windows.nsis.installerHooks = "./installer-hooks.nsh";
  writeFileSync(confPath, JSON.stringify(conf, null, 2) + "\n");
  const cases = langs
    .map((l, i) => `    ${i ? "${ElseIf}" : "${If}"} $LANGUAGE == ${l.data._meta.langid}\n      StrCpy $0 "${l.code}"`)
    .join("\n");
  // $UpdateMode = 1 quando o próprio Larafim roda o instalador (/UPDATE): a
  // tela de idioma não aparece e a desinstalação da versão antiga não pode
  // apagar a chave.
  const nsh = `; Gerado por scripts/i18n.mjs sync — não edite à mão.
; Grava o idioma escolhido no instalador; o Larafim usa na primeira vez que abre.
; Atualizações feitas pelo app ($UpdateMode = 1) não mexem na chave.
!include "LogicLib.nsh"

!macro NSIS_HOOK_POSTINSTALL
  \${If} $UpdateMode <> 1
    StrCpy $0 "${SOURCE}"
${cases}
    \${EndIf}
    WriteRegStr HKCU "Software\\Larafim" "Language" $0
  \${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  \${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\\Larafim"
  \${EndIf}
!macroend
`;
  writeFileSync(join(root, "app", "src-tauri", "installer-hooks.nsh"), nsh);
  console.log(`instalador: ${langs.map((l) => l.code).join(", ")}`);
} else {
  console.error("uso: node scripts/i18n.mjs check|keys|sync|fill <código>");
  process.exitCode = 2;
}
