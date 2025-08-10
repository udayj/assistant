use crate::prices::item_prices::Description;
use crate::quotation::{QuotationResponse, QuotedItem};
use ::image::codecs::jpeg::JpegDecoder;
use ::image::io::Reader as ImageReader;
use printpdf::*;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const PAGE_WIDTH_MM: f64 = 210.0;
const PAGE_HEIGHT_MM: f64 = 297.0;
const MARGIN_MM: f64 = 10.0;
const TABLE_START_Y: f64 = 200.0;
const ROW_HEIGHT_MM: f64 = 10.0;
const MIN_ROW_HEIGHT_MM: f64 = 10.0;
const MAX_CHARS_PER_LINE: usize = 45; // Adjust based on your font size and column width

pub fn create_quotation_pdf(
    quotation_number: &str,
    date: &str,
    quotation: &QuotationResponse,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let (doc, page1, layer1) = PdfDocument::new(
        "Quotation",
        Mm(PAGE_WIDTH_MM),
        Mm(PAGE_HEIGHT_MM),
        "Layer 1",
    );

    let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;

    let mut current_page = page1;
    let mut current_layer = doc.get_page(current_page).get_layer(layer1);
    let mut current_y = TABLE_START_Y;

    // Add header to first page
    add_header_to_page(&current_layer, quotation_number, date, &font)?;

    // Table column positions
    let col_item = MARGIN_MM;
    let col_qty = 120.0;
    let col_rate = 140.0;
    let col_amount = 170.0;
    let table_width = col_amount + 30.0 - col_item;

    // Add table headers
    add_table_headers(
        &current_layer,
        &font_bold,
        current_y,
        col_item,
        col_qty,
        col_rate,
        col_amount,
        table_width,
    );
    current_y -= ROW_HEIGHT_MM;

    // Process items
    for item in &quotation.items {
        let mut extras = Vec::new();
        if item.loading_frls > 0.0 {
            extras.push("frls".to_string());
        }
        if item.loading_pvc > 0.0 {
            extras.push("pvc".to_string());
        }

        let description = item.product.get_description(extras);
        let lines = wrap_text(&description, MAX_CHARS_PER_LINE);
        let row_height = (lines.len() as f64 * 8.0).max(MIN_ROW_HEIGHT_MM);

        // Check if we need a new page
        if current_y - row_height < 50.0 {
            // Leave space for totals
            // Add current page table border
            //draw_table_border(&current_layer, col_item, TABLE_START_Y, table_width, TABLE_START_Y - current_y - ROW_HEIGHT_MM);

            // Create new page
            let (new_page, new_layer) =
                doc.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "Layer");
            current_page = new_page;
            current_layer = doc.get_page(current_page).get_layer(new_layer);
            current_y = 230.0;

            // Add header to new page
            add_image_only_to_page(&current_layer)?;

            // Add table headers on new page
            add_table_headers(
                &current_layer,
                &font_bold,
                current_y,
                col_item,
                col_qty,
                col_rate,
                col_amount,
                table_width,
            );
            current_y -= ROW_HEIGHT_MM;
        }

        // Draw row border
        draw_row_border(&current_layer, col_item, current_y, table_width, row_height);

        // Add item data
        add_item_row(
            &current_layer,
            &font,
            &lines,
            item,
            current_y,
            row_height,
            col_item,
            col_qty,
            col_rate,
            col_amount,
        );

        current_y -= row_height;
    }

    // Close the table with bottom border
    //draw_table_border(&current_layer, col_item, TABLE_START_Y, table_width, TABLE_START_Y - current_y);

    // Add totals section
    current_y -= 20.0;
    add_totals_section(
        &current_layer,
        &font,
        &font_bold,
        quotation,
        current_y,
        col_amount + 30.0,
    );

    // Save PDF
    doc.save(&mut BufWriter::new(File::create(filename)?))?;
    Ok(())
}

fn add_image_only_to_page(layer: &PdfLayerReference) -> Result<(), Box<dyn std::error::Error>> {
    // Load and add header image only
    let img_info = ImageReader::open("assets/header.jpg")?.decode()?.to_rgb8();
    let (width_px, height_px) = (img_info.width() as f32, img_info.height() as f32);

    let mut image_file = std::fs::File::open(Path::new("assets/header.jpg"))?;
    let img = Image::try_from(JpegDecoder::new(&mut image_file).unwrap()).unwrap();

    let scale = PAGE_WIDTH_MM / (width_px * 25.4 / 96.0) as f64;
    let scaled_height_mm = (height_px * scale as f32 * 25.4 / 96.0) as f64;

    let transform = ImageTransform {
        translate_x: Some(Mm(0.0)),
        translate_y: Some(Mm(PAGE_HEIGHT_MM - scaled_height_mm)),
        rotate: None,
        scale_x: Some(scale),
        scale_y: Some(scale),
        dpi: Some(96.0),
    };

    img.add_to_layer(layer.clone(), transform);
    Ok(())
}

fn add_header_to_page(
    layer: &PdfLayerReference,
    quotation_number: &str,
    date: &str,
    font: &IndirectFontRef,
) -> Result<(), Box<dyn std::error::Error>> {
    // Load and add header image
    let img_info = ImageReader::open("assets/header.jpg")?.decode()?.to_rgb8();
    let (width_px, height_px) = (img_info.width() as f32, img_info.height() as f32);

    let mut image_file = std::fs::File::open(Path::new("assets/header.jpg"))?;
    let img = Image::try_from(JpegDecoder::new(&mut image_file).unwrap()).unwrap();

    let scale = PAGE_WIDTH_MM / (width_px * 25.4 / 96.0) as f64;
    let scaled_height_mm = (height_px * scale as f32 * 25.4 / 96.0) as f64;

    let transform = ImageTransform {
        translate_x: Some(Mm(0.0)),
        translate_y: Some(Mm(PAGE_HEIGHT_MM - scaled_height_mm)),
        rotate: None,
        scale_x: Some(scale),
        scale_y: Some(scale),
        dpi: Some(96.0),
    };

    img.add_to_layer(layer.clone(), transform);

    // Add quotation details
    layer.use_text(quotation_number, 10.0, Mm(MARGIN_MM), Mm(220.0), font);
    layer.use_text(date, 10.0, Mm(157.0), Mm(220.0), font);

    layer.use_text(
        "Thank you for enquiry. Please find the quotation below for your consideration",
        10.0,
        Mm(MARGIN_MM),
        Mm(210.0),
        font,
    );

    Ok(())
}

fn add_table_headers(
    layer: &PdfLayerReference,
    font_bold: &IndirectFontRef,
    y_pos: f64,
    col_item: f64,
    col_qty: f64,
    col_rate: f64,
    col_amount: f64,
    table_width: f64,
) {
    // Add header text with proper padding from lines
    layer.use_text("Item", 10.0, Mm(col_item + 2.0), Mm(y_pos - 4.0), font_bold); // Changed from -2.0 to -4.0
    layer.use_text(
        "Qty (Mtr)",
        10.0,
        Mm(col_qty + 2.0),
        Mm(y_pos - 4.0),
        font_bold,
    );
    layer.use_text(
        "Rate/mtr.",
        10.0,
        Mm(col_rate + 2.0),
        Mm(y_pos - 4.0),
        font_bold,
    );
    layer.use_text(
        "Amount Rs.",
        10.0,
        Mm(col_amount + 2.0),
        Mm(y_pos - 4.0),
        font_bold,
    );

    // Draw header border
    draw_horizontal_line(layer, col_item, y_pos + 5.0, table_width);
    draw_horizontal_line(layer, col_item, y_pos - ROW_HEIGHT_MM, table_width);
    draw_vertical_line(layer, col_item, y_pos + 5.0, ROW_HEIGHT_MM + 10.0);
    draw_vertical_line(layer, col_qty, y_pos + 5.0, ROW_HEIGHT_MM + 10.0);
    draw_vertical_line(layer, col_rate, y_pos + 5.0, ROW_HEIGHT_MM + 10.0);
    draw_vertical_line(layer, col_amount, y_pos + 5.0, ROW_HEIGHT_MM + 10.0);
    draw_vertical_line(
        layer,
        col_item + table_width,
        y_pos + 5.0,
        ROW_HEIGHT_MM + 10.0,
    );
}

fn add_item_row(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    description_lines: &[String],
    item: &QuotedItem,
    y_pos: f64,
    row_height: f64,
    col_item: f64,
    col_qty: f64,
    col_rate: f64,
    col_amount: f64,
) {
    // Add description (multi-line) - start from top of row with proper padding
    for (i, line) in description_lines.iter().enumerate() {
        layer.use_text(
            line,
            9.0,
            Mm(col_item + 2.0),
            Mm(y_pos - 4.0 - (i as f64 * 8.0)), // Changed from -2.0 to -4.0
            font,
        );
    }

    // Center other values vertically in the row with proper padding
    let text_y = y_pos - (row_height / 2.0) - 2.0; // Changed from -1.0 to -2.0

    layer.use_text(
        &format!("{:.0}", item.quantity_mtrs),
        9.0,
        Mm(col_qty + 2.0),
        Mm(text_y),
        font,
    );

    layer.use_text(
        &format!("{:.2}", item.price),
        9.0,
        Mm(col_rate + 2.0),
        Mm(text_y),
        font,
    );

    layer.use_text(
        &format!("{:.2}", item.amount),
        9.0,
        Mm(col_amount + 2.0),
        Mm(text_y),
        font,
    );
}

fn add_totals_section(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
    quotation: &QuotationResponse,
    mut y_pos: f64,
    right_align_x: f64,
) {
    let label_x = right_align_x - 60.0;
    let value_x = right_align_x - 5.0;

    // Sub Total
    layer.use_text("Sub Total:", 10.0, Mm(label_x), Mm(y_pos), font_bold);
    layer.use_text(
        &format!("Rs.{:.2}", quotation.basic_total),
        10.0,
        Mm(value_x - get_text_width(&format!("Rs.{:.2}", quotation.basic_total))),
        Mm(y_pos),
        font_bold,
    );

    // Delivery Charges (if applicable)
    if quotation.delivery_charges > 0.0 {
        y_pos -= ROW_HEIGHT_MM;
        layer.use_text("Delivery Charges:", 10.0, Mm(label_x), Mm(y_pos), font);
        layer.use_text(
            &format!("Rs.{:.2}", quotation.delivery_charges),
            10.0,
            Mm(value_x - get_text_width(&format!("Rs.{:.2}", quotation.delivery_charges))),
            Mm(y_pos),
            font,
        );
    }

    // GST
    y_pos -= ROW_HEIGHT_MM;
    layer.use_text("GST @ 18%:", 10.0, Mm(label_x), Mm(y_pos), font);
    layer.use_text(
        &format!("Rs.{:.2}", quotation.taxes),
        10.0,
        Mm(value_x - get_text_width(&format!("Rs.{:.2}", quotation.taxes))),
        Mm(y_pos),
        font,
    );

    // Total
    y_pos -= ROW_HEIGHT_MM;
    layer.use_text("Total:", 10.0, Mm(label_x), Mm(y_pos), font_bold);
    layer.use_text(
        &format!("Rs.{:.2}", quotation.grand_total),
        10.0,
        Mm(value_x - get_text_width(&format!("Rs.{:.2}", quotation.grand_total))),
        Mm(y_pos),
        font_bold,
    );
}

fn wrap_text(text: &str, max_chars_per_line: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.len() + word.len() + 1 <= max_chars_per_line {
            if !current_line.is_empty() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        } else {
            if !current_line.is_empty() {
                lines.push(current_line.clone());
                current_line.clear();
            }

            if word.len() > max_chars_per_line {
                // Split very long words
                let mut remaining = word;
                while remaining.len() > max_chars_per_line {
                    lines.push(remaining[..max_chars_per_line].to_string());
                    remaining = &remaining[max_chars_per_line..];
                }
                current_line = remaining.to_string();
            } else {
                current_line = word.to_string();
            }
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    lines
}

fn draw_row_border(layer: &PdfLayerReference, x: f64, y: f64, width: f64, height: f64) {
    let col_qty = 120.0;
    let col_rate = 140.0;
    let col_amount = 170.0;

    // Horizontal lines
    draw_horizontal_line(layer, x, y - height, width);

    // Vertical lines for columns
    draw_vertical_line(layer, x, y, height);
    draw_vertical_line(layer, col_qty, y, height);
    draw_vertical_line(layer, col_rate, y, height);
    draw_vertical_line(layer, col_amount, y, height);
    draw_vertical_line(layer, x + width, y, height);
}

fn draw_horizontal_line(layer: &PdfLayerReference, x: f64, y: f64, width: f64) {
    let line = Line {
        points: vec![
            (Point::new(Mm(x), Mm(y)), false),
            (Point::new(Mm(x + width), Mm(y)), false),
        ],
        is_closed: false,
        has_fill: false,
        has_stroke: true,
        is_clipping_path: false,
    };

    layer.add_shape(line);
}

fn draw_vertical_line(layer: &PdfLayerReference, x: f64, y: f64, height: f64) {
    let line = Line {
        points: vec![
            (Point::new(Mm(x), Mm(y)), false),
            (Point::new(Mm(x), Mm(y - height)), false),
        ],
        is_closed: false,
        has_fill: false,
        has_stroke: true,
        is_clipping_path: false,
    };

    layer.add_shape(line);
}

fn get_text_width(text: &str) -> f64 {
    // Approximate text width calculation (you might want to use a more accurate method)
    // This is a rough estimation based on character count
    text.len() as f64 * 2.5 // Adjust multiplier based on your font size
}
