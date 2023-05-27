use std::env::args;

use pdf_writer::{Content, Name, Obj, PdfWriter, Rect, Ref, Str};

fn main() {
    let file = args().nth(1).expect("Usage: [file.pdf]");

    let (image_rect, image_content, image_fonts) = extract_image(&file).unwrap();

    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let contents_id = Ref::new(4);
    let image_id = Ref::new(5);
    let font_id = Ref::new(6);

    let font_name = Name(b"F1");
    let image_name = Name(b"myimg");

    let mut writer = PdfWriter::new();
    writer.catalog(catalog_id).pages(page_tree_id);
    writer.pages(page_tree_id).kids([page_id]).count(1);

    let page_rect = Rect::new(0.0, 0.0, 612.0, 792.0);

    {
        // create image object
        let mut xobj = writer.form_xobject(image_id, &image_content);

        xobj.bbox(image_rect);

        let mut resources = xobj.resources();
        let mut fonts_dict = resources.fonts();

        // reference fonts
        for (i, (name, _font)) in image_fonts.iter().enumerate() {
            let id = Ref::new(i as i32 + 7);
            fonts_dict.pair(Name(&name), id);
        }
    }

    {
        // create a new page
        let mut page = writer.page(page_id);
        page.parent(page_tree_id)
            .contents(contents_id)
            .media_box(page_rect);

        let mut resources = page.resources();
        resources.x_objects().pair(image_name, image_id);
        resources.fonts().pair(font_name, font_id);
    }

    // embed fonts
    for (i, (_name, font)) in image_fonts.iter().enumerate() {
        let id = Ref::new(i as i32 + 7);
        let mut descriptor = writer.font_descriptor(id);
        for (key, value) in font.into_iter() {
            // just copy all elements
            convert_obj(value, descriptor.insert(Name(&key)));
        }
    }
    writer.type1_font(font_id).base_font(Name(b"Helvetica"));

    let desired_width = 500.0;
    let scale = desired_width / image_rect.x2;
    let x = (page_rect.x2 - image_rect.x2 * scale) / 2.0;
    let y = (page_rect.y2 - image_rect.y2 * scale) / 2.0;

    println!(
        "Image at {x}, {y} with {}, {}",
        image_rect.x2 * scale,
        image_rect.y2 * scale
    );

    let mut page_content = Content::new();
    page_content
        .begin_text()
        .set_font(font_name, 14.0)
        .next_line(108.0, 734.0)
        .show(Str(b"Hello World"))
        .end_text()
        .save_state()
        .transform([1.0, 0.0, 0.0, 1.0, x, y]) // position
        .transform([scale, 0.0, 0.0, scale, 0.0, 0.0]) // scale
        .x_object(image_name)
        .restore_state();
    writer.stream(contents_id, &page_content.finish());

    std::fs::write("out.pdf", writer.finish()).unwrap();
}

fn extract_image(file: &str) -> lopdf::Result<(Rect, Vec<u8>, Vec<(Vec<u8>, lopdf::Dictionary)>)> {
    let doc = lopdf::Document::load(file)?;
    println!("{doc:#?}");
    let page_id = doc.get_pages()[&1];
    println!("{page_id:?}");
    let meta = doc.get_dictionary(page_id)?;
    println!("{meta:#?}");
    let media_box = meta
        .get(b"MediaBox")?
        .as_array()?
        .into_iter()
        .map(|o| o.as_float().unwrap())
        .collect::<Vec<_>>();
    let rect = Rect::new(media_box[0], media_box[1], media_box[2], media_box[3]);
    println!("{:?}", meta.get(b"MediaBox")?);
    let fonts = doc.get_page_fonts(page_id);
    println!("{fonts:#?}");

    let objects = doc.get_page_contents(page_id);
    println!("{objects:?}");

    let content = doc.get_page_content(page_id)?;

    Ok((
        rect,
        content,
        fonts.into_iter().map(|(k, v)| (k, v.clone())).collect(),
    ))
}

/// Convert from one library to another
fn convert_obj<'a>(from: &lopdf::Object, into: Obj<'a>) {
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
                convert_obj(elem, arr.push());
            }
        }
        Object::Dictionary(v) => {
            let mut dict = into.dict();
            for (key, val) in v.into_iter() {
                convert_obj(val, dict.insert(Name(key)))
            }
        }
        Object::Stream(_v) => todo!("streams are not yet implemented"),
        Object::Reference(v) => into.primitive(Ref::new(v.0 as _)),
    }
}
