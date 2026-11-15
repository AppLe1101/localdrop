export type Fx = "auto" | "on" | "off";
const KEY = "ld-fx";

export function getFx(): Fx {
  try {
    const v = localStorage.getItem(KEY);
    return v === "on" || v === "off" ? v : "auto";
  } catch {
    return "auto";
  }
}

function softwareRenderer(): boolean {
  try {
    const c = document.createElement("canvas");
    const gl = (c.getContext("webgl") || c.getContext("experimental-webgl")) as WebGLRenderingContext | null;
    if (!gl) return true;
    const ext = gl.getExtension("WEBGL_debug_renderer_info");
    const r = ext ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)) : "";
    return /swiftshader|basic render|llvmpipe|software/i.test(r);
  } catch {
    return false;
  }
}

export function applyFx(f: Fx = getFx()) {
  const flat = f === "off" || (f === "auto" && softwareRenderer());
  document.documentElement.dataset.fx = flat ? "flat" : "glass";
}

export function setFx(f: Fx) {
  try { localStorage.setItem(KEY, f); } catch {}
  applyFx(f);
}