// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : ui/src/lib/forgefmt.ts
//  Author : Praneeth
//  License: MIT
//  Purpose: PacketForge byte rendering in standard notations:
//             text  - printable characters, C escapes for the rest (\r \n \x1B)
//                     or Hercules-style {0D} when compatibility is on
//             hex   - "48 65 6C" (uppercase, space separated)
//             dump  - `hexdump -C` layout (offset, 16 bytes, |ascii|)
//             dec / bin - decimal and binary bytes
// =============================================================================

export type Encoding = "utf8" | "latin1" | "ascii";
export type Format = "text" | "hex" | "mixed" | "dump" | "dec" | "bin";

/** A run of output text; `ctrl` marks escape sequences (rendered dimmed). */
export interface Seg {
  t: string;
  ctrl: boolean;
}

const HEX = Array.from({ length: 256 }, (_, i) => i.toString(16).toUpperCase().padStart(2, "0"));

function esc(b: number, hercules: boolean): string {
  if (hercules) return `{${HEX[b]}}`;
  switch (b) {
    case 0x0d: return "\\r";
    case 0x0a: return "\\n";
    case 0x09: return "\\t";
    case 0x00: return "\\0";
    case 0x1b: return "\\e";
    default: return `\\x${HEX[b]}`;
  }
}

/** Length of a valid UTF-8 sequence starting at i, or 0. */
function utf8Len(d: Uint8Array, i: number): number {
  const b = d[i];
  const need = b >= 0xf0 && b <= 0xf4 ? 4 : b >= 0xe0 ? 3 : b >= 0xc2 && b <= 0xdf ? 2 : 0;
  if (!need || i + need > d.length) return 0;
  for (let k = 1; k < need; k++) if ((d[i + k] & 0xc0) !== 0x80) return 0;
  return need;
}

const dec = new TextDecoder("utf-8");

/**
 * Text with escapes. Line breaks are shown as escapes *and* kept as real
 * breaks when `breakLines` is set, so multi-line data stays readable.
 */
export function toText(d: Uint8Array, enc: Encoding, hercules: boolean, breakLines = false): Seg[] {
  const out: Seg[] = [];
  let plain = "";
  const flush = () => {
    if (plain) out.push({ t: plain, ctrl: false });
    plain = "";
  };
  for (let i = 0; i < d.length; i++) {
    const b = d[i];
    if (b >= 0x20 && b < 0x7f) {
      plain += String.fromCharCode(b);
      continue;
    }
    if (b >= 0x80) {
      if (enc === "utf8") {
        const n = utf8Len(d, i);
        if (n) {
          plain += dec.decode(d.subarray(i, i + n));
          i += n - 1;
          continue;
        }
      } else if (enc === "latin1" && b >= 0xa0) {
        plain += String.fromCharCode(b);
        continue;
      }
    }
    flush();
    out.push({ t: esc(b, hercules), ctrl: true });
    if (breakLines && b === 0x0a) out.push({ t: "\n", ctrl: false });
  }
  flush();
  return out;
}

export function toHex(d: Uint8Array, max = Infinity): string {
  const n = Math.min(d.length, max);
  let s = "";
  for (let i = 0; i < n; i++) s += (i ? " " : "") + HEX[d[i]];
  return n < d.length ? `${s} …` : s;
}

export function toDec(d: Uint8Array): string {
  return Array.from(d, (b) => String(b)).join(" ");
}

export function toBin(d: Uint8Array): string {
  return Array.from(d, (b) => b.toString(2).padStart(8, "0")).join(" ");
}

/** `hexdump -C` style lines. */
export function toDump(d: Uint8Array): string[] {
  const lines: string[] = [];
  for (let off = 0; off < d.length; off += 16) {
    const row = d.subarray(off, off + 16);
    let hex = "";
    for (let i = 0; i < 16; i++) {
      hex += i < row.length ? HEX[row[i]].toLowerCase() + " " : "   ";
      if (i === 7) hex += " ";
    }
    const ascii = Array.from(row, (b) => (b >= 0x20 && b < 0x7f ? String.fromCharCode(b) : ".")).join("");
    lines.push(`${off.toString(16).padStart(8, "0")}  ${hex} |${ascii}|`);
  }
  return lines;
}

export function fromBase64(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** Search bytes for a pattern (returns true when found). */
export function contains(hay: Uint8Array, needle: Uint8Array, caseInsensitive: boolean): boolean {
  if (!needle.length) return true;
  const lower = (b: number) => (caseInsensitive && b >= 0x41 && b <= 0x5a ? b + 32 : b);
  outer: for (let i = 0; i + needle.length <= hay.length; i++) {
    for (let k = 0; k < needle.length; k++) if (lower(hay[i + k]) !== lower(needle[k])) continue outer;
    return true;
  }
  return false;
}

/** Parse "0D 0A" / "0x0D,0x0A" / "0D0A" into bytes (null when invalid). */
export function parseHex(s: string): Uint8Array | null {
  const out: number[] = [];
  for (const raw of s.split(/[\s,:;-]+/).filter(Boolean)) {
    const tok = raw.replace(/^0x/i, "");
    if (!tok.length || tok.length % 2 || !/^[0-9a-f]+$/i.test(tok)) return null;
    for (let i = 0; i < tok.length; i += 2) out.push(parseInt(tok.slice(i, i + 2), 16));
  }
  return new Uint8Array(out);
}

export function timeLabel(mode: "none" | "absolute" | "relative" | "delta", t: number, start: number, prev: number | null): string {
  switch (mode) {
    case "absolute": {
      const d = new Date(t);
      return `${d.toLocaleTimeString([], { hour12: false })}.${String(Math.floor(t % 1000)).padStart(3, "0")}`;
    }
    case "relative":
      return `${((t - start) / 1000).toFixed(3)}s`;
    case "delta":
      return prev == null ? "+0 ms" : `+${Math.round(t - prev)} ms`;
    default:
      return "";
  }
}
