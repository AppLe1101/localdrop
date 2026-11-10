export type Kind = "files" | "text";
export type Direction = "send" | "receive";
export type Decision = "reject" | "acceptOnce" | "acceptAndTrust";

export interface Peer { id: string; name: string; addr: string; fp: string | null }
export type Visibility = "off" | "trustedOnly" | "everyone";
export interface MyInfo { name: string; fpShort: string; visibility: Visibility }
export interface SettingsData { deviceName: string; downloadDir: string; visibility: Visibility; autostart: boolean; quitOnClose: boolean; hotkey: string; }
export interface SendResult {
  id: number;
  outcome: "sent" | "rejected" | "nameConflict" | "failed";
}
export interface DeviceInfo { fingerprint: string; name: string; trusted: boolean }
export interface HistoryRecord {
  id: number;
  ts: number;
  direction: Direction;
  name: string;
  size: number;
  peer: string;
  status: "done" | "rejected" | "failed";
  path: string | null;
  error: string | null;
  text: string | null;
  batch: string | null;
}

export type CoreEvent =
  | { type: "incoming"; id: number; sender: string; kind: Kind; name: string; count: number; size: number;
    preview: string | null; fpShort: string; code: string; knownName: string | null; timeoutSecs: number }
  | { type: "textReceived"; id: number; text: string }
  | { type: "peerCode"; id: number; code: string; newDevice: boolean; trusted: boolean }
  | { type: "started"; id: number; direction: Direction; name: string; size: number; peerName: string }
  | { type: "progress"; id: number; done: number; total: number }
  | { type: "finished"; id: number; path: string | null }
  | { type: "rejected"; id: number }
  | { type: "failed"; id: number; error: string };

export interface Incoming {
  id: number; sender: string; kind: Kind; name: string; count: number; size: number;
  preview: string | null; fpShort: string; code: string; knownName: string | null;
  timeoutSecs: number;
  expiresAt: number;
}

export interface Transfer {
  id: number;
  direction: Direction;
  name: string;
  size: number;
  done: number;
  status: "waiting" | "active" | "done" | "failed" | "rejected";
  awaiting?: boolean;
  peerId?: string;
  peerName?: string;
  code?: string;
  newDevice?: boolean;
  error?: string;
  path?: string | null;
}

export interface WebSession {
  id: string;
  kind: "share" | "receive";
  title: string;
  files: number;
  totalSize: number;
  expiresAt: number;
  maxDownloads: number | null;
  downloads: number;
  uploads: number;
  receivedBytes: number;
  maxBytes: number | null;
  port: number;
  hosts: string[];
  pin: string | null;
}

export type WebEvent =
  | { type: "changed" }
  | { type: "downloaded"; session: string; name: string; size: number; ip: string }
  | { type: "uploaded"; session: string; name: string; size: number; path: string; ip: string }
  | { type: "closed"; session: string; title: string; reason: "expired" | "limit" | "manual" | "attempts" }
  | { type: "pinLocked"; session: string; title: string; ip: string; failures: number };