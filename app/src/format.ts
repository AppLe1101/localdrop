export const human = (n: number) => {
  let v = n;
  for (const u of ["Б", "КБ", "МБ", "ГБ"]) {
    if (v < 1024) return `${v.toFixed(v < 10 && u !== "Б" ? 1 : 0)} ${u}`;
    v /= 1024;
  }
  return `${v.toFixed(1)} ТБ`;
};
export const hue = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 360, 7);
export const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p;
export const initial = (s: string) => (s.trim().slice(0, 1) || "•").toUpperCase();