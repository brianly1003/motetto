// Languages notes can be written or read in: the same codes as LANGUAGES in
// crates/motetto-core/src/summarize.rs (Rust refuses any other).
export const NOTE_LANGUAGES = ['ar','bn','cs','da','de','el','en','es','fa','fi','fr','he','hi','hu','id','it','ja','km','ko','ms','nl','no','pl','pt','ro','ru','sv','ta','th','tl','tr','uk','ur','vi','zh'] as const;

function display(locale: string, code: string) {
  try { return new Intl.DisplayNames([locale], {type: 'language'}).of(code) ?? code; } catch { return code; }
}
/** Each language in its own words ("Melayu", "Tiếng Việt", "中文"), as its readers look for it. */
export function nativeName(code: string) {
  const name = display(code, code);
  return name.charAt(0).toLocaleUpperCase(code) + name.slice(1);
}
export const englishName = (code: string) => display('en', code);

const fold = (text: string) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();
/** Typed text matches a code, its own name or its English name, ignoring case and accents. */
export function matches(code: string, query: string) {
  const q = fold(query.trim());
  return !q || code === q || fold(nativeName(code)).includes(q) || fold(englishName(code)).includes(q);
}

/** The computer's languages that notes can be read in, in its order ("ms-MY" → "ms"). */
export function systemLanguages(list: readonly string[] = typeof navigator === 'undefined' ? [] : navigator.languages) {
  const known = new Set<string>(NOTE_LANGUAGES);
  return [...new Set(list.map(tag => tag.split('-')[0].toLowerCase()).map(code => code === 'fil' ? 'tl' : code === 'nb' || code === 'nn' ? 'no' : code))].filter(code => known.has(code));
}

/** Null notes language means the call's own language; mixed calls have no single original. */
export function originalNotesLanguage(written?: string | null, call?: string | null) {
  if (written) return written;
  const languages = [...new Set((call ?? '').split(',').map(code => code.trim()).filter(Boolean))];
  return languages.length === 1 ? languages[0] : null;
}

/** A translation has the notes' exact lines (motetto_core::translate), so it shows their ticks. */
export function withTicks(translated: string, notes: string) {
  const ticks = notes.split('\n');
  return translated.split('\n').map((line, i) => {
    const tick = /^(\s*[-*] )\[([ xX])\]/.exec(ticks[i] ?? '');
    return tick ? line.replace(/^(\s*[-*] )\[[ xX]\]/, `$1[${tick[2]}]`) : line;
  }).join('\n');
}
