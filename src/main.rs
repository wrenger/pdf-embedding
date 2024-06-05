use std::collections::{BTreeMap, HashMap};
use std::env::args;
use std::path::Path;

use pdf_writer::{Chunk, Content, Dict, Finish, Name, Obj, Pdf, Rect, Ref, Str};

static USAGE: &str = "Usage: [file.pdf] [page]";

fn main() {
    let mut alloc = Ref::new(1);
    let file = args().nth(1).expect(USAGE);
    let page = args().nth(2).and_then(|s| s.parse().ok()).expect(USAGE);

    // Extract a pdf page
    let img = PdfImage::extract(Path::new(&file), page).unwrap();

    // Embed the pdf
    let (img_chunk, img_id) = embed_image(&img);
    // Renumber the chunk so that we can embed it into our existing workflow, and also make sure
    // to update `img_id`.
    let mut map = HashMap::new();
    let img_chunk = img_chunk.renumber(|old| *map.entry(old).or_insert_with(|| alloc.bump()));
    let img_id = map.get(&img_id).unwrap();

    // create a new PDF with the image
    let font_name = Name(b"F1");
    let img_name = Name(b"myimg");
    let mut pdf = Pdf::new();
    let page_tree_id = alloc.bump();
    pdf.catalog(alloc.bump()).pages(page_tree_id);
    let contents_id = alloc.bump();
    let page_rect = Rect::new(0.0, 0.0, 612.0, 792.0);

    let page_id = alloc.bump();
    pdf.pages(page_tree_id).kids([page_id]).count(1);

    let mut page = pdf.page(page_id);
    page.parent(page_tree_id)
        .contents(contents_id)
        .media_box(page_rect);

    let font_id = alloc.bump();
    let mut resources = page.resources();
    resources.x_objects().pair(img_name, img_id);
    resources.fonts().pair(font_name, font_id);
    resources.finish();
    page.finish();

    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));

    let desired_width = 500.0;
    let scale = desired_width / img.rect.x2;
    let x = (page_rect.x2 - img.rect.x2 * scale) / 2.0;
    let y = (page_rect.y2 - img.rect.y2 * scale) / 2.0;

    println!(
        "Image at {x}, {y} with {}, {}",
        img.rect.x2 * scale,
        img.rect.y2 * scale
    );

    let mut page_content = Content::new();
    page_content
        .begin_text()
        .set_font(font_name, 14.0)
        .next_line(108.0, 734.0)
        .show(Str(b"Hello World"))
        .end_text();

    // Add our graphic
    page_content
        .save_state()
        .transform([1.0, 0.0, 0.0, 1.0, x, y]) // position
        .transform([scale, 0.0, 0.0, scale, 0.0, 0.0]) // scale
        .x_object(img_name)
        .restore_state();

    pdf.stream(contents_id, &page_content.finish());

    pdf.extend(&img_chunk);

    std::fs::write("out.pdf", pdf.finish()).unwrap();
}

fn embed_image(img: &PdfImage) -> (Chunk, Ref) {
    let mut alloc = Ref::new(1);
    let mut chunk = Chunk::new();

    let image_id = alloc.bump();

    let mut indirects = {
        // create image object
        let mut xobj = chunk.form_xobject(image_id, &img.stream.content);

        xobj.bbox(img.rect);

        let mut indirects = Vec::new();
        for (key, val) in img.stream.dict.iter() {
            write_obj(&mut alloc, xobj.insert(Name(key)), val, &mut indirects)
        }

        // add ressources
        let res_obj = xobj.insert(Name(b"Resources")).start();
        write_dict(&mut alloc, res_obj, &img.ressources, &mut indirects);
        indirects
    };

    // recursively copy indirectly referenced objects
    let mut remainder = Vec::new();
    while !indirects.is_empty() {
        for (id, obj) in &indirects {
            match obj {
                lopdf::Object::Stream(s) => {
                    let mut stream = chunk.stream(*id, &s.content);
                    for (key, val) in s.dict.iter() {
                        write_obj(&mut alloc, stream.insert(Name(key)), val, &mut remainder)
                    }
                }
                lopdf::Object::Reference(r) => {
                    let obj = img.objects.get(&r).unwrap();
                    // Streams are always indirect -> skip indirection
                    if let lopdf::Object::Stream(_) = obj {
                        remainder.push((*id, obj));
                    } else {
                        write_obj(&mut alloc, chunk.indirect(*id), obj, &mut remainder);
                    }
                }
                _ => panic!("Invalid obj"),
            }
        }
        indirects.clear();
        std::mem::swap(&mut remainder, &mut indirects);
    }

    (chunk, image_id)
}

struct PdfImage {
    rect: Rect,
    stream: lopdf::Stream,
    ressources: lopdf::Dictionary,
    objects: BTreeMap<lopdf::ObjectId, lopdf::Object>,
}
impl PdfImage {
    fn extract(file: &Path, page: usize) -> lopdf::Result<Self> {
        let doc = lopdf::Document::load(file)?;
        let page_id = *doc
            .get_pages()
            .get(&(page as u32))
            .ok_or(lopdf::Error::PageNumberNotFound(page as _))?;
        println!("\n\nPage: {page_id:?}");

        let meta = doc.get_dictionary(page_id)?;
        println!("\n\nMeta: {meta:#?}");

        let media_box = meta
            .get(b"MediaBox")?
            .as_array()?
            .into_iter()
            .map(|o| o.as_float().unwrap())
            .collect::<Vec<_>>();
        let rect = Rect::new(media_box[0], media_box[1], media_box[2], media_box[3]);
        println!("{:?}", meta.get(b"MediaBox")?);

        // Find ressources
        let ressources = match doc.get_page_resources(page_id) {
            (Some(ressources), _) => ressources.clone(),
            (None, ids) if ids.len() > 0 => doc.get_dictionary(ids[0])?.clone(),
            _ => Default::default(),
        };

        println!("\n\nRessources: {ressources:#?}");

        let contents = doc.get_page_contents(page_id);
        assert_eq!(contents.len(), 1);
        let lopdf::Object::Stream(stream) = doc.get_object(contents[0])? else {
            return Err(lopdf::Error::PageNumberNotFound(1));
        };

        Ok(Self {
            rect,
            stream: stream.clone(),
            ressources,
            objects: doc.objects,
        })
    }
}

fn write_dict<'a>(
    alloc: &mut Ref,
    mut into: Dict<'_>,
    from: &'a lopdf::Dictionary,
    indirects: &mut Vec<(Ref, &'a lopdf::Object)>,
) {
    for (key, val) in from.into_iter() {
        write_obj(alloc, into.insert(Name(key)), val, indirects)
    }
}

fn write_obj<'a>(
    alloc: &mut Ref,
    into: Obj<'_>,
    from: &'a lopdf::Object,
    indirects: &mut Vec<(Ref, &'a lopdf::Object)>,
) {
    use lopdf::Object;
    match from {
        Object::Null => {}
        Object::Boolean(v) => into.primitive(v),
        Object::Integer(v) => into.primitive(*v as i32),
        Object::Real(v) => into.primitive(v),
        Object::Name(v) => into.primitive(Name(&v)),
        Object::String(v, _) => into.primitive(Str(&v)),
        Object::Array(v) => {
            let mut arr = into.array();
            for elem in v {
                write_obj(alloc, arr.push(), elem, indirects);
            }
        }
        Object::Dictionary(v) => write_dict(alloc, into.dict(), v, indirects),
        Object::Reference(_) | Object::Stream(_) => {
            let id = alloc.bump();
            indirects.push((id, from));
            into.primitive(id);
        }
    }
}
