export interface CardInput {
  svg: string;
  title: string;
  subtitle: string;
  expiresAt: number;
  pin: boolean;
}

const W = 900;
const H = 1200;
const FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif';

function roundRect(c: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  c.beginPath();
  c.moveTo(x + r, y);
  c.arcTo(x + w, y, x + w, y + h, r);
  c.arcTo(x + w, y + h, x, y + h, r);
  c.arcTo(x, y + h, x, y, r);
  c.arcTo(x, y, x + w, y, r);
  c.closePath();
}

function fit(c: CanvasRenderingContext2D, text: string, maxW: number) {
  if (c.measureText(text).width <= maxW) return text;
  let s = text;
  while (s.length > 1 && c.measureText(s + "…").width > maxW) s = s.slice(0, -1);
  return s + "…";
}

function loadSvg(svg: string): Promise<HTMLImageElement> {
  const fixed = svg.includes("xmlns=") ? svg : svg.replace("<svg", '<svg xmlns="http://www.w3.org/2000/svg"');
  return new Promise((res, rej) => {
    const img = new Image();
    img.onload = () => res(img);
    img.onerror = () => rej(new Error("Не удалось отрисовать QR-код"));
    img.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(fixed);
  });
}

function expiryText(ts: number) {
  if (ts < Date.now()) return "Срок действия истёк";
  const d = new Date(ts);
  const t = d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  const date = d.toLocaleDateString("ru-RU", {
    day: "numeric", month: "long", year: d.getFullYear() !== new Date().getFullYear() ? "numeric" : undefined,
  });
  return `Действует до ${t}, ${date}`;
}

export async function renderQrCard(i: CardInput): Promise<HTMLCanvasElement> {
  const img = await loadSvg(i.svg);
  const cv = document.createElement("canvas");
  cv.width = W;
  cv.height = H;
  const c = cv.getContext("2d")!;

  const g = c.createLinearGradient(0, 0, W, H);
  g.addColorStop(0, "#5ab0ff");
  g.addColorStop(1, "#0a5cf0");
  c.fillStyle = g;
  c.fillRect(0, 0, W, H);

  c.strokeStyle = "rgba(255,255,255,.12)";
  c.lineWidth = 3;
  for (const r of [300, 440, 580, 720]) {
    c.beginPath();
    c.arc(W / 2, 480, r, 0, Math.PI * 2);
    c.stroke();
  }

  // logo
  c.textAlign = "center";
  c.textBaseline = "alphabetic";
  c.fillStyle = "#fff";
  c.font = `600 40px ${FONT}`;
  c.fillText("LocalDrop", W / 2 + 26, 108);
  const tw = c.measureText("LocalDrop").width;
  const lx = W / 2 + 26 - tw / 2 - 40;
  c.beginPath(); c.arc(lx, 94, 7, 0, Math.PI * 2); c.fill();
  c.strokeStyle = "rgba(255,255,255,.85)"; c.lineWidth = 3;
  c.beginPath(); c.arc(lx, 94, 17, 0, Math.PI * 2); c.stroke();
  c.strokeStyle = "rgba(255,255,255,.5)";
  c.beginPath(); c.arc(lx, 94, 27, 0, Math.PI * 2); c.stroke();

  // QR
  const card = 640;
  const cx = (W - card) / 2;
  const cy = 170;
  c.save();
  c.shadowColor = "rgba(0,40,120,.35)";
  c.shadowBlur = 60;
  c.shadowOffsetY = 24;
  c.fillStyle = "#fff";
  roundRect(c, cx, cy, card, card, 56);
  c.fill();
  c.restore();
  c.imageSmoothingEnabled = false;
  c.drawImage(img, cx + 40, cy + 40, card - 80, card - 80);

  // subtitles
  c.fillStyle = "#fff";
  c.font = `700 52px ${FONT}`;
  c.fillText(fit(c, i.title, W - 120), W / 2, 905);
  c.globalAlpha = 0.85;
  c.font = `400 32px ${FONT}`;
  c.fillText(fit(c, i.subtitle, W - 120), W / 2, 958);
  c.globalAlpha = 1;

  // expires
  const label = expiryText(i.expiresAt);
  c.font = `600 38px ${FONT}`;
  const pw = Math.min(W - 80, c.measureText(label).width + 80);
  c.fillStyle = "rgba(255,255,255,.2)";
  roundRect(c, (W - pw) / 2, 996, pw, 84, 42);
  c.fill();
  c.fillStyle = "#fff";
  c.fillText(fit(c, label, pw - 60), W / 2, 1052);

  c.globalAlpha = 0.75;
  c.font = `400 28px ${FONT}`;
  c.fillText("Наведите камеру телефона. Нужна та же сеть Wi-Fi", W / 2, 1130);
  if (i.pin) c.fillText("Потребуется PIN-код", W / 2, 1166);
  c.globalAlpha = 1;

  return cv;
}

export const canvasBlob = (cv: HTMLCanvasElement) =>
  new Promise<Blob>((res, rej) => cv.toBlob((b) => (b ? res(b) : rej(new Error("PNG не создан"))), "image/png"));