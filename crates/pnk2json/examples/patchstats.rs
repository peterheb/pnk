//! Corpus scan: incremental-save patch statistics per document.
fn main() {
    let mut tot_a = 0u64;
    let mut tot_d = 0u64;
    let mut files_a = 0u32;
    let mut files_d = 0u32;
    for arg in std::env::args().skip(1) {
        let path = std::path::Path::new(&arg);
        let Ok(ctx) = pnk2json::ctx::Ctx::open(path) else { continue };
        let (a, d) = (ctx.loaded.patches_applied, ctx.loaded.patches_dropped);
        if a > 0 { files_a += 1; }
        if d > 0 { files_d += 1; }
        if a > 0 || d > 0 {
            println!("{a:6} applied {d:6} dropped  {arg}");
        }
        tot_a += a;
        tot_d += d;
    }
    eprintln!("TOTAL applied={tot_a} dropped={tot_d} files-with-applied={files_a} files-with-dropped={files_d}");
}
