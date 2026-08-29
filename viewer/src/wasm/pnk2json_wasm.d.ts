/* tslint:disable */
/* eslint-disable */

export function convert(bytes: Uint8Array): string;

/**
 * Markdown fallback dump from raw bytes.
 */
export function convert_markdown(bytes: Uint8Array): string;

/**
 * Pretty-printed JSON (debug/inspection use; ~3x larger than compact).
 */
export function convert_pretty(bytes: Uint8Array): string;

/**
 * Raw bytes of the media asset with the given DataInfo id (decimal string),
 * from the last successfully converted document. `None` when the asset has
 * no DataInfo entry or its `Data/` bytes are absent (remote/unmaterialized).
 */
export function media_bytes(data_id: string): Uint8Array | undefined;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly convert: (a: number, b: number) => [number, number, number, number];
    readonly convert_markdown: (a: number, b: number) => [number, number, number, number];
    readonly convert_pretty: (a: number, b: number) => [number, number, number, number];
    readonly media_bytes: (a: number, b: number) => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
