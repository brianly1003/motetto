// One editor's writes share their latest version, even when a slow save overlaps typing.
export function scriptWriter(name: string, text: string, modified: number | null,
  save: (name: string, text: string, base: number | null) => Promise<number>) {
  let pending = Promise.resolve();
  return (next: string) => {
    const result = pending.then(async () => {
      if (next !== text) { modified = await save(name, next, modified); text = next; }
      return modified;
    });
    pending = result.then(() => {}, () => {});
    return result;
  };
}
