import { useSyncExternalStore } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdState =
  | { s: "idle" }
  | { s: "checking" }
  | { s: "uptodate" }
  | { s: "available"; version: string; notes: string }
  | { s: "downloading"; done: number; total: number }
  | { s: "installing" }
  | { s: "error"; msg: string };

let state: UpdState = { s: "idle" };
let update: Update | null = null;
const subs = new Set<() => void>();

function set(s: UpdState) {
  state = s;
  subs.forEach((f) => f());
}

export const useUpdater = () =>
  useSyncExternalStore(
    (f) => {
      subs.add(f);
      return () => subs.delete(f);
    },
    () => state,
  );

const AUTO_KEY = "ld-autoupdate";
export const getAutoCheck = () => {
  try {
    return localStorage.getItem(AUTO_KEY) !== "off";
  } catch {
    return true;
  }
};
export const setAutoCheck = (on: boolean) => {
  try {
    localStorage.setItem(AUTO_KEY, on ? "on" : "off");
  } catch {
  }
};

export async function checkForUpdates(silent = false) {
  if (state.s === "checking" || state.s === "downloading" || state.s === "installing") return;
  if (!silent) set({ s: "checking" });
  try {
    const u = await check();
    if (u) {
      update = u;
      set({ s: "available", version: u.version, notes: u.body ?? "" });
    } else if (!silent) {
      set({ s: "uptodate" });
    }
  } catch (e) {
    if (!silent) set({ s: "error", msg: String(e) });
  }
}

export async function installUpdate() {
  if (!update) return;
  let total = 0;
  let done = 0;
  try {
    set({ s: "downloading", done: 0, total: 0 });
    await update.downloadAndInstall((e) => {
      if (e.event === "Started") total = e.data.contentLength ?? 0;
      else if (e.event === "Progress") {
        done += e.data.chunkLength;
        set({ s: "downloading", done, total });
      } else if (e.event === "Finished") set({ s: "installing" });
    });
    await relaunch();
  } catch (e) {
    set({ s: "error", msg: String(e) });
  }
}