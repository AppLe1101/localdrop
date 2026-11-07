export type Theme = "system" | "light" | "dark";
const KEY = "ld-theme";

export function getTheme(): Theme {
    try {
        const v = localStorage.getItem(KEY);
        return v === "light" || v === "dark" ? v : "system";
    } catch {
        return "system";
    }
}

export function applyTheme(t: Theme) {
    const r = document.documentElement;
    if (t === "system") r.removeAttribute("data-theme");
    else r.setAttribute("data-theme", t);
}

export function setTheme(t: Theme) {
    try {
        localStorage.setItem(KEY, t);
    } catch {
    }
    applyTheme(t);
}

export function initTheme() {
    applyTheme(getTheme());
    const sync = () => applyTheme(getTheme());
    window.addEventListener("storage", (e) => e.key === "ld-theme" && sync());
    window.addEventListener("focus", sync);
}