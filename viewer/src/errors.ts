// Human explanations for the converter's rejections. Messages mirror
// crates/pnk2json/src/loader.rs (iwadump layer-tagged errors): encrypted
// (.iwph/.iwpv2) and legacy bundle markers are refused there, before any
// parsing — everything stays client-side.

export interface FriendlyError {
  title: string;
  body: string;
  detail: string;
}

export function mapError(err: unknown, filename: string): FriendlyError {
  const message = err instanceof Error ? err.message : String(err);

  if (/encrypted iWork document|\.iwp(h|v)/i.test(message)) {
    return {
      title: "This file is password-protected",
      body: "The document is encrypted (an .iwph/.iwpv2 marker is inside the container). Encrypted iWork files need the password to decrypt — and pnk never asks for or transmits anything, so it politely refuses instead of guessing.",
      detail: message,
    };
  }
  if (/legacy iWork document/i.test(message)) {
    return {
      title: "This is a legacy iWork file",
      body: "The file is a pre-iWork '13 document bundle (index.apxl / index.xml era). Open it once in a current version of Pages, Numbers or Keynote and re-save it; pnk reads the modern snappy-IWA format only.",
      detail: message,
    };
  }
  if (/not a readable ZIP/i.test(message)) {
    return {
      title: "That doesn't look like an iWork document",
      body: "pnk opens .pages, .numbers and .key files (modern packages are ZIP containers). This file isn't one — maybe the wrong file got dropped?",
      detail: message,
    };
  }
  return {
    title: `Couldn't open ${filename}`,
    body: "The document could not be decoded. If it opens fine in Pages/Numbers/Keynote, this is a pnk bug — the raw converter message below helps fix it.",
    detail: message,
  };
}

export function renderErrorCard(err: FriendlyError, mount: HTMLElement): void {
  const card = document.createElement("div");
  card.className = "error-card";
  const title = document.createElement("div");
  title.className = "error-title";
  title.textContent = err.title;
  const body = document.createElement("p");
  body.textContent = err.body;
  const detail = document.createElement("code");
  detail.className = "error-detail";
  detail.textContent = err.detail;
  card.append(title, body, detail);
  mount.appendChild(card);
}