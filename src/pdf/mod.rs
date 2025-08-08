use crate::prices::item_prices::Description;
use crate::quotation::QuotationResponse;
use ::image::codecs::jpeg::JpegDecoder;
use ::image::io::Reader as ImageReader;
use printpdf::*;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub fn create_quotation_pdf(
    quotation_number: &str,
    date: &str,
    quotation: &QuotationResponse,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (doc, page1, layer1) = PdfDocument::new("Quotation", Mm(210.0), Mm(297.0), "Layer 1");
    let current_layer = doc.get_page(page1).get_layer(layer1);

    // Load and add header image
    // Load the image to get dimensions
    let img_info = ImageReader::open("assets/header.jpg")?.decode()?.to_rgb8();

    // Get image dimensions in pixels
    let (width_px, height_px) = (img_info.width() as f32, img_info.height() as f32);

    // Load image for PDF
    let mut image_file = std::fs::File::open(Path::new("assets/header.jpg"))?;
    let img = Image::try_from(JpegDecoder::new(&mut image_file).unwrap()).unwrap();

    // Define page dimensions in mm
    let page_width_mm = 210.0; // A4 width
    let page_height_mm = 297.0; // A4 height

    // Calculate scaling to fit width while maintaining aspect ratio
    let scale = page_width_mm / (width_px * 25.4 / 96.0); // Convert px to mm using 96 DPI

    // Calculate height after scaling
    let scaled_height_mm = (height_px * scale * 25.4 / 96.0);

    // Create image transform
    let transform = ImageTransform {
        translate_x: Some(Mm(0.0)),
        translate_y: Some(Mm((page_height_mm - scaled_height_mm) as f64)),
        rotate: None,
        scale_x: Some(scale as f64),
        scale_y: Some(scale as f64),
        dpi: Some(96.0),
    };

    // Add the header image
    img.add_to_layer(current_layer.clone(), transform);

    // Fonts
    let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;

    // Quotation details
    current_layer.use_text(quotation_number, 10.0, Mm(10.0), Mm(220.0), &font);
    current_layer.use_text(date, 10.0, Mm(157.0), Mm(220.0), &font);

    current_layer.use_text(
        "Thank you for enquiry. Please find the quotation below for your consideration",
        10.0,
        Mm(10.0),
        Mm(205.0),
        &font,
    );
    // Table headers
    let header_y = Mm(190.0);
    current_layer.use_text("Item", 10.0, Mm(10.0), header_y, &font_bold);
    current_layer.use_text("Qty (Mtr)", 10.0, Mm(120.0), header_y, &font_bold);
    current_layer.use_text("Rate/mtr.", 10.0, Mm(140.0), header_y, &font_bold);
    current_layer.use_text("Amount", 10.0, Mm(170.0), header_y, &font_bold);

    // Table items
    let mut y_pos = 180.0;
    for item in &quotation.items {
        let mut extras = Vec::new();
        if item.loading_frls > 0.0 {
            extras.push("frls".to_string());
        }
        if item.loading_pvc > 0.0 {
            extras.push("pvc".to_string());
        }
        //let item_desc = format!("{:?}", ); // Simple debug format
        current_layer.use_text(
            &item.product.get_description(extras),
            9.0,
            Mm(10.0),
            Mm(y_pos),
            &font,
        );
        current_layer.use_text(
            &format!("{:.0}", item.quantity_mtrs),
            9.0,
            Mm(120.0),
            Mm(y_pos),
            &font,
        );
        current_layer.use_text(
            &format!("{:.2}", item.price),
            9.0,
            Mm(140.0),
            Mm(y_pos),
            &font,
        );
        current_layer.use_text(
            &format!("{:.2}", item.amount),
            9.0,
            Mm(170.0),
            Mm(y_pos),
            &font,
        );
        y_pos -= 10.0;
    }

    // Totals
    y_pos -= 10.0;
    current_layer.use_text(
        &format!("Sub Total: Rs.{:.2}", quotation.basic_total),
        10.0,
        Mm(140.0),
        Mm(y_pos),
        &font_bold,
    );

    if quotation.delivery_charges > 0.0 {
        y_pos -= 10.0;
        current_layer.use_text(
            &format!("Delivery Charges: Rs.{:.2}", quotation.delivery_charges),
            10.0,
            Mm(140.0),
            Mm(y_pos),
            &font,
        );
    }
    y_pos -= 10.0;
    current_layer.use_text(
        &format!("GST @ 18%: Rs.{:.2}", quotation.taxes),
        10.0,
        Mm(140.0),
        Mm(y_pos),
        &font,
    );
    y_pos -= 10.0;
    current_layer.use_text(
        &format!("Total: Rs.{:.2}", quotation.grand_total),
        10.0,
        Mm(140.0),
        Mm(y_pos),
        &font_bold,
    );

    // Save PDF
    doc.save(&mut BufWriter::new(File::create(filename)?))?;
    Ok(())
}
