// Ambient declarations for build-time assets and the minified pdf.js entry.
declare module "*.txt" {
  const text: string;
  export default text;
}
declare module "pdfjs-dist/build/pdf.min.mjs" {
  export * from "pdfjs-dist/types/src/pdf";
}
