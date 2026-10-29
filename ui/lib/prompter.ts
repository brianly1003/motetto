// Measure the rendered words: a reading step must fit the actual width, font and text size.
export type ReadingPage = { word: number; top: number; height: number };
export type ReadingRow = { word: number; top: number };

/**
 * The rendered rows of a paragraph: the first word on each and how far down it is. Layout
 * positions (offsetTop), not on-screen ones: words rising in are moved by their animation.
 */
export function readingRows(text: HTMLElement): ReadingRow[] {
  const height = parseFloat(getComputedStyle(text).lineHeight);
  const words = [...text.querySelectorAll<HTMLElement>('[data-word]')];
  const first = words[0]?.offsetTop ?? 0;
  const rows: ReadingRow[] = [];
  for (const word of words) {
    const top = word.offsetTop - first;
    if (!rows.length || top - rows[rows.length - 1].top > height / 2)
      rows.push({ word: Number(word.dataset.word), top });
  }
  return rows.length ? rows : [{ word: 0, top: 0 }];
}

export function readingPages(text: HTMLElement, rows: number): ReadingPage[] {
  const height = parseFloat(getComputedStyle(text).lineHeight);
  const lines = readingRows(text);
  const pages: ReadingPage[] = [];
  for (let i = 0; i < lines.length; i += rows)
    pages.push({ word: i ? lines[i].word : 0, top: lines[i].top, height: Math.min(rows, lines.length - i) * height + 4 });
  return pages;
}

/** Words left on a row when the next row rolls into view: the highlight trails your voice. */
const ROLL_AT = 3;

/**
 * What the strip shows while you read `word`, given the first word of the rows shown before
 * (`start`). The text keeps still while you read; only when the highlight is near the end of the
 * bottom row does it roll down one row, so the next line is there as your eyes reach it.
 * `from`/`to` are the first shown word and the first hidden one after them (the row above is
 * hidden: clipped, its letters' tails would show); `last` means the paragraph's end is shown.
 */
export function readingWindow(rows: ReadingRow[], perPage: number, word: number, lineHeight: number, start?: number) {
  const row = Math.max(0, rows.findLastIndex(r => r.word <= word));
  const next = rows[row + 1];
  const need = perPage > 1 && next && next.word - word <= ROLL_AT ? row + 1 : row;
  const before = start === undefined ? 0 : Math.max(0, rows.findLastIndex(r => r.word <= start));
  const first = Math.min(row, Math.max(before, need - perPage + 1));
  const shown = Math.min(perPage, rows.length - first);
  return {
    page: Math.floor(row / perPage), total: Math.ceil(rows.length / perPage),
    start: rows[first].word, from: first ? rows[first].word : 0,
    to: rows[first + shown]?.word ?? Infinity, last: first + shown >= rows.length,
    top: rows[first].top, height: shown * lineHeight + 4,
  };
}
