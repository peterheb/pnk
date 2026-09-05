// Shared render context: object URLs for media assets, created from the raw
// bytes served by the wasm `media_bytes(dataId)` export. All local — blob
// URLs, no network.

const EXT_MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  bmp: "image/bmp",
  svg: "image/svg+xml",
  tif: "image/tiff",
  tiff: "image/tiff",
  heic: "image/heic",
  heif: "image/heif",
  pdf: "application/pdf",
  mp4: "video/mp4",
  m4v: "video/mp4",
  mov: "video/quicktime",
  mp3: "audio/mpeg",
  m4a: "audio/mp4",
  aac: "audio/aac",
  wav: "audio/wav",
  aiff: "audio/aiff",
  aif: "audio/aiff",
};

export class ViewerCtx {
  private urls = new Map<string, string>();
  // Raw bytes of vector media (PDF/AI) kept for pdf.js rasterization; other
  // kinds live only behind their blob URL.
  private vectorBytes = new Map<string, Uint8Array>();

  /** Register bytes for a DataInfo id; later `url()` calls hand back a blob URL. */
  addMedia(dataId: string, bytes: Uint8Array, fileName?: string): void {
    // Copy into a fresh ArrayBuffer: the wasm heap slice is invalidated by the
    // next convert() call.
    const copy = new Uint8Array(bytes.byteLength);
    copy.set(bytes);
    const ext = fileName?.split(".").pop()?.toLowerCase() ?? "";
    const blob = new Blob([copy], { type: EXT_MIME[ext] ?? "application/octet-stream" });
    this.urls.set(dataId, URL.createObjectURL(blob));
    if (ext === "pdf" || ext === "ai") this.vectorBytes.set(dataId, copy);
  }

  url(dataId: string): string | undefined {
    return this.urls.get(dataId);
  }

  /** Bytes of a PDF/AI asset, for pdf.js; undefined for other media. */
  bytes(dataId: string): Uint8Array | undefined {
    return this.vectorBytes.get(dataId);
  }

  /** Revoke every object URL (when a new document replaces the current one). */
  dispose(): void {
    for (const u of this.urls.values()) URL.revokeObjectURL(u);
    this.urls.clear();
    this.vectorBytes.clear();
  }
}