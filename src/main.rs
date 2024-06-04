use std::{collections::BTreeMap, env::args};

use pdf_writer::{Content, Dict, Name, Obj, Pdf, Rect, Ref, Str};

fn main() {
    let mut ref_counter = Ref::new(1);
    let file = args().nth(1).expect("Usage: [file.pdf]");

    let (image_rect, image_content, ressources, objects) = extract_image(&file).unwrap();

    let mut pdf = Pdf::new();
    let page_tree_id = ref_counter.bump();
    pdf.catalog(ref_counter.bump()).pages(page_tree_id);

    let page_id = ref_counter.bump();
    let image_id = ref_counter.bump();
    let font_id = ref_counter.bump();

    let font_name = Name(b"F1");
    let image_name = Name(b"myimg");

    pdf.pages(page_tree_id).kids([page_id]).count(1);

    let page_rect = Rect::new(0.0, 0.0, 612.0, 792.0);

    let mut indirects = {
        // create image object
        let mut xobj = pdf.form_xobject(image_id, &image_content.content);

        xobj.bbox(image_rect);

        let mut indirects = Vec::new();
        for (key, val) in image_content.dict.iter() {
            convert_obj(
                &mut ref_counter,
                xobj.insert(Name(key)),
                val,
                &objects,
                &mut indirects,
            )
        }

        // add ressources
        let res_obj = xobj.insert(Name(b"Resources")).start();
        convert_dict(
            &mut ref_counter,
            res_obj,
            &ressources,
            &objects,
            &mut indirects,
        );
        indirects
    };

    // copy indirectly referenced objects
    let mut remainder = Vec::new();
    while !indirects.is_empty() {
        while let Some((id, obj)) = indirects.pop() {
            if let lopdf::Object::Stream(s) = obj {
                let mut stream = pdf.stream(id, &s.content);
                for (key, val) in s.dict.iter() {
                    convert_obj(
                        &mut ref_counter,
                        stream.insert(Name(key)),
                        val,
                        &objects,
                        &mut remainder,
                    )
                }
            } else {
                convert_obj(
                    &mut ref_counter,
                    pdf.indirect(id),
                    obj,
                    &objects,
                    &mut remainder,
                );
            }
        }
        std::mem::swap(&mut remainder, &mut indirects);
    }

    let contents_id = ref_counter.bump();
    {
        // create a new page
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .contents(contents_id)
            .media_box(page_rect);

        let mut resources = page.resources();
        resources.x_objects().pair(image_name, image_id);
        resources.fonts().pair(font_name, font_id);
    }

    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));

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
    pdf.stream(contents_id, &page_content.finish());

    std::fs::write("out.pdf", pdf.finish()).unwrap();
}

fn extract_image(
    file: &str,
) -> lopdf::Result<(
    Rect,
    lopdf::Stream,
    lopdf::Dictionary,
    BTreeMap<lopdf::ObjectId, lopdf::Object>,
)> {
    let doc = lopdf::Document::load(file)?;
    let page_id = doc.get_pages()[&1];
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

    Ok((rect, stream.clone(), ressources, doc.objects))
}

fn convert_dict<'a, 'b: 'a>(
    ref_counter: &mut Ref,
    mut into: Dict<'a>,
    from: &'b lopdf::Dictionary,
    objects: &'b BTreeMap<lopdf::ObjectId, lopdf::Object>,
    indirects: &mut Vec<(Ref, &'b lopdf::Object)>,
) {
    for (key, val) in from.into_iter() {
        convert_obj(ref_counter, into.insert(Name(key)), val, objects, indirects)
    }
}

/// Convert from one library to another
fn convert_obj<'a, 'b: 'a>(
    ref_counter: &mut Ref,
    into: Obj<'a>,
    from: &'b lopdf::Object,
    objects: &'b BTreeMap<lopdf::ObjectId, lopdf::Object>,
    indirects: &mut Vec<(Ref, &'b lopdf::Object)>,
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
                convert_obj(ref_counter, arr.push(), elem, objects, indirects);
            }
        }
        Object::Dictionary(v) => convert_dict(ref_counter, into.dict(), v, objects, indirects),
        Object::Stream(_v) => {
            let id = ref_counter.bump();
            indirects.push((id, from));
            into.primitive(id);
        }
        Object::Reference(v) => {
            let obj = objects.get(v).unwrap();
            let id = ref_counter.bump();
            indirects.push((id, obj));
            into.primitive(id)
        }
    }
}
