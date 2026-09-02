//! Agent K corpus survey: TSA.CaptionInfoArchive objects per document.
fn main() {
    let (mut docs, mut with, mut total) = (0u32, 0u32, 0u64);
    for arg in std::env::args().skip(1) {
        let Ok((_d, loaded)) = pnk2json::loader::open_document(std::path::Path::new(&arg)) else {
            continue;
        };
        docs += 1;
        let n = loaded
            .records
            .values()
            .filter(|r| r.name.as_deref() == Some("TSA.CaptionInfoArchive"))
            .count() as u64;
        if n > 0 {
            with += 1;
            total += n;
            if with <= 8 {
                println!("{n:4}  {arg}");
            }
        }
    }
    println!("docs={docs} with_captions={with} caption_objects={total}");
}
