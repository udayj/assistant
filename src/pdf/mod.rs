use crate::prices::item_prices::Description;
use crate::quotation::{QuotationResponse, QuotedItem};
use ::image::codecs::jpeg::JpegDecoder;
use ::image::io::Reader as ImageReader;
use printpdf::*;
use std::fs;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

const PAGE_WIDTH_MM: f64 = 210.0;
const PAGE_HEIGHT_MM: f64 = 297.0;
const MARGIN_MM: f64 = 10.0;
const BASE_TABLE_START_Y: f64 = 200.0;
const ROW_HEIGHT_MM: f64 = 10.0;
const MIN_ROW_HEIGHT_MM: f64 = 10.0;
const MAX_CHARS_PER_LINE: usize = 60;
const TO_SECTION_LINE_SPACING: f64 = 5.0;
const SECOND_PAGE_START_Y: f64 = 230.0;
const TC_SECTION_LINE_SPACING: f64 = 5.0;
const MAX_TOTALS_SECTION_HEIGHT: f64 = 28.0;
const FOOTER_Y_MM: f64 = 5.0;

#[derive(Debug, Clone, Copy)]
pub enum DocumentType {
    Quotation,
    ProformaInvoice,
}

impl DocumentType {
    pub fn get_header_text(&self) -> &'static str {
        match self {
            Self::Quotation => "QUOTATION",
            Self::ProformaInvoice => "PROFORMA INVOICE",
        }
    }

    pub fn get_ref_prefix(&self) -> &'static str {
        match self {
            Self::Quotation => "Q",
            Self::ProformaInvoice => "PI",
        }
    }
}

pub fn create_quotation_pdf(
    quotation_number: &str,
    date: &str,
    quotation: &QuotationResponse,
    filename: &str,
    document_type: DocumentType,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all("artifacts")?;
    let (doc, page1, layer1) = PdfDocument::new(
        "Quotation",
        Mm(PAGE_WIDTH_MM),
        Mm(PAGE_HEIGHT_MM),
        "Layer 1",
    );

    let font = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_bold = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;

    let to_section_height = quotation
        .to
        .as_ref()
        .map(|lines| (lines.len() + 1) as f64 * TO_SECTION_LINE_SPACING + 5.0) // line height + spacing
        .unwrap_or(0.0);

    let table_start_y = BASE_TABLE_START_Y - to_section_height;
    let mut current_page = page1;
    let mut current_layer = doc.get_page(current_page).get_layer(layer1);
    let mut current_y = table_start_y;

    // Add header to first page
    add_header_to_page(
        &current_layer,
        quotation_number,
        date,
        &quotation.to,
        &font,
        document_type,
    )?;

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

        let description = format!(
            "{}",
            item.product.get_description(extras)
        );
        let lines = wrap_text(&description, MAX_CHARS_PER_LINE);
        let row_height = (lines.len() as f64 * 8.0).max(MIN_ROW_HEIGHT_MM);

        // Check if we need a new page
        if current_y - row_height < MAX_TOTALS_SECTION_HEIGHT + 10.0 {
            // Leave space for totals
            // Add current page table border
            //draw_table_border(&current_layer, col_item, TABLE_START_Y, table_width, TABLE_START_Y - current_y - ROW_HEIGHT_MM);

            // Create new page
            let (new_page, new_layer) =
                doc.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "Layer");
            current_page = new_page;
            current_layer = doc.get_page(current_page).get_layer(new_layer);
            current_y = SECOND_PAGE_START_Y;

            // Add header to new page
            add_image_only_to_page(&current_layer, &font)?;

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
            col_item,
            col_qty,
            col_rate,
            col_amount,
        );

        current_y -= row_height;
    }

    // Add totals section
    current_y -= 15.0;
    let totals_start_y = current_y;
    add_totals_section(
        &current_layer,
        &font,
        &font_bold,
        quotation,
        current_y,
        col_amount + 40.0,
    );

    let totals_height = if quotation.delivery_charges > 0.0 {
        MAX_TOTALS_SECTION_HEIGHT
    } else {
        MAX_TOTALS_SECTION_HEIGHT - 7.0
    };
    current_y = totals_start_y - totals_height;

    let terms_section_height = quotation
        .terms_and_conditions
        .as_ref()
        .map(|terms| ((terms.len() + 1) as f64 * TC_SECTION_LINE_SPACING) + 15.0) // terms lines + header + spacing
        .unwrap_or(0.0);

    // Add terms and conditions with space check
    if let Some(terms) = &quotation.terms_and_conditions {
        // Check if terms fit on current page
        if current_y - terms_section_height < 10.0 {
            // 20mm bottom margin
            // Create new page for terms
            let (new_page, new_layer) =
                doc.add_page(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), "Layer");
            current_layer = doc.get_page(new_page).get_layer(new_layer);
            add_image_only_to_page(&current_layer, &font)?;
            current_y = SECOND_PAGE_START_Y; // Start high on new page
        } else {
            current_y -= 5.0; // Space after totals on same page
        }

        add_terms_and_conditions(&current_layer, &font, &font_bold, terms, current_y);
    }

    // Save PDF
    let full_filename = format!("artifacts/{}", filename);
    doc.save(&mut BufWriter::new(File::create(full_filename)?))?;
    Ok(())
}

fn add_image_only_to_page(layer: &PdfLayerReference, font: &IndirectFontRef) -> Result<(), Box<dyn std::error::Error>> {
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

    // Add marketing footer
    add_marketing_footer(layer, font);

    Ok(())
}

fn add_header_to_page(
    layer: &PdfLayerReference,
    quotation_number: &str,
    date: &str,
    to: &Option<Vec<String>>,
    font: &IndirectFontRef,
    document_type: DocumentType,
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

    let header_text = document_type.get_header_text();
    let page_center_x = PAGE_WIDTH_MM / 2.0;

    let header_x = page_center_x - (header_text.len() as f64 * 2.0);
    layer.use_text(header_text, 12.0, Mm(header_x), Mm(240.0), font);
    let text_width = header_text.len() as f64 * 2.8;
    draw_horizontal_line(layer, header_x, 238.0, text_width);

    // Add quotation details
    let quotation_reference = format!("Ref: {}", quotation_number);
    layer.use_text(quotation_reference, 10.0, Mm(MARGIN_MM), Mm(220.0), font);
    layer.use_text(date, 10.0, Mm(157.0), Mm(220.0), font);

    let mut current_y = 220.0;
    if let Some(to_lines) = to {
        current_y -= 7.0; // Space after date
        layer.use_text("To:", 10.0, Mm(MARGIN_MM), Mm(current_y), font);
        current_y -= TO_SECTION_LINE_SPACING;

        for line in to_lines {
            layer.use_text(line, 10.0, Mm(MARGIN_MM), Mm(current_y), font);
            current_y -= TO_SECTION_LINE_SPACING;
        }
        current_y -= TO_SECTION_LINE_SPACING; // Extra space after "to" section
    } else {
        current_y -= 10.0; // Standard spacing when no "to" section
    }

    let mut introduction_text =
        "Thank you for enquiry. Please find the quotation below for your consideration:-";
    match document_type {
        DocumentType::ProformaInvoice => introduction_text = "Please find the details below:-",
        _ => {}
    }
    layer.use_text(introduction_text, 10.0, Mm(MARGIN_MM), Mm(current_y), font);

    // Add marketing footer
    add_marketing_footer(layer, font);

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
    col_item: f64,
    col_qty: f64,
    col_rate: f64,
    col_amount: f64,
) {
    // Add description (multi-line) - start from top of row with proper padding
    let mut row_y_pos = y_pos;
    for (i, line) in description_lines.iter().enumerate() {
        row_y_pos = y_pos - 4.0 - (i as f64 * 8.0);
        layer.use_text(line, 9.0, Mm(col_item + 2.0), Mm(row_y_pos), font);
    }

    // Center other values vertically in the row with proper padding
    // let text_y = y_pos - (row_height / 2.0) - 2.0; // Changed from -1.0 to -2.0
    let text_y = row_y_pos;
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
    let row_separation = 7.0;
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
        y_pos -= row_separation;
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
    y_pos -= row_separation;
    layer.use_text("GST @ 18%:", 10.0, Mm(label_x), Mm(y_pos), font);
    layer.use_text(
        &format!("Rs.{:.2}", quotation.taxes),
        10.0,
        Mm(value_x - get_text_width(&format!("Rs.{:.2}", quotation.taxes))),
        Mm(y_pos),
        font,
    );

    // Total
    y_pos -= row_separation;
    layer.use_text("Total:", 10.0, Mm(label_x), Mm(y_pos), font_bold);
    layer.use_text(
        &format!("Rs.{:.2}", quotation.grand_total),
        10.0,
        Mm(value_x - get_text_width(&format!("Rs.{:.2}", quotation.grand_total))),
        Mm(y_pos),
        font_bold,
    );
}

fn add_terms_and_conditions(
    layer: &PdfLayerReference,
    font: &IndirectFontRef,
    font_bold: &IndirectFontRef,
    terms: &[String],
    mut y_pos: f64,
) {
    layer.use_text(
        "Terms & Conditions:",
        10.0,
        Mm(MARGIN_MM),
        Mm(y_pos),
        font_bold,
    );
    y_pos -= TC_SECTION_LINE_SPACING;

    for term in terms {
        layer.use_text(term, 9.0, Mm(MARGIN_MM), Mm(y_pos), font);
        y_pos -= TC_SECTION_LINE_SPACING;
    }
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

fn add_marketing_footer(layer: &PdfLayerReference, font: &IndirectFontRef) {
    let grey_color = Color::Rgb(Rgb::new(0.5, 0.5, 0.5, None)); // 50% grey
    let blue_color = Color::Rgb(Rgb::new(0.27, 0.51, 0.71, None)); // Steel blue (70, 130, 180)

    let prefix_text = "Prepared using ";
    let emphasis_text = "AGL Intelligent Commercial Automation";

    // Calculate text widths more accurately for 8pt font
    let prefix_width = prefix_text.len() as f64 * 1.4; // Better estimate for 8pt font
    let emphasis_width = emphasis_text.len() as f64 * 1.4;
    let total_width = prefix_width + emphasis_width;

    // Center the entire text block
    let start_x = (PAGE_WIDTH_MM - total_width) / 2.0;

    // Add grey prefix text
    layer.set_fill_color(grey_color);
    layer.use_text(prefix_text, 8.0, Mm(start_x), Mm(FOOTER_Y_MM), font);

    // Add blue emphasis text (positioned after prefix text)
    layer.set_fill_color(blue_color);
    layer.use_text(emphasis_text, 8.0, Mm(start_x + prefix_width), Mm(FOOTER_Y_MM), font);

    // Reset to black color for any subsequent text
    layer.set_fill_color(Color::Rgb(Rgb::new(0.0, 0.0, 0.0, None)));
}

#[cfg(test)]
mod pdf_tests {
    use super::*;
    use crate::prices::item_prices::*;
    use crate::quotation::*;

    #[test]
    fn test_pdf_generation() {
        let test_quotation = QuotationResponse {
            items: vec![
                QuotedItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(
                        Flexible {
                            core_size: "4".to_string(),
                            sqmm: "2.5".to_string(),
                            flexible_type: FlexibleType::FR,
                        },
                    ))),
                    brand: "polycab".to_string(),
                    quantity_mtrs: 100.0,
                    price: 250.60,
                    amount: 25060.00,
                    loading_frls: 0.05,
                    loading_pvc: 0.03,
                },
                QuotedItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(
                        Flexible {
                            core_size: "4".to_string(),
                            sqmm: "2.5".to_string(),
                            flexible_type: FlexibleType::FR,
                        },
                    ))),
                    brand: "polycab".to_string(),
                    quantity_mtrs: 100.0,
                    price: 250.60,
                    amount: 25060.00,
                    loading_frls: 0.05,
                    loading_pvc: 0.03,
                },
                QuotedItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(
                        Flexible {
                            core_size: "4".to_string(),
                            sqmm: "2.5".to_string(),
                            flexible_type: FlexibleType::FR,
                        },
                    ))),
                    brand: "polycab".to_string(),
                    quantity_mtrs: 100.0,
                    price: 250.60,
                    amount: 25060.00,
                    loading_frls: 0.05,
                    loading_pvc: 0.03,
                },
                QuotedItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(
                        Flexible {
                            core_size: "4".to_string(),
                            sqmm: "2.5".to_string(),
                            flexible_type: FlexibleType::FR,
                        },
                    ))),
                    brand: "polycab".to_string(),
                    quantity_mtrs: 100.0,
                    price: 250.60,
                    amount: 25060.00,
                    loading_frls: 0.05,
                    loading_pvc: 0.03,
                },
                QuotedItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::Flexible(
                        Flexible {
                            core_size: "4".to_string(),
                            sqmm: "2.5".to_string(),
                            flexible_type: FlexibleType::FR,
                        },
                    ))),
                    brand: "polycab".to_string(),
                    quantity_mtrs: 100.0,
                    price: 250.60,
                    amount: 25060.00,
                    loading_frls: 0.05,
                    loading_pvc: 0.03,
                },
                QuotedItem {
                    product: Product::Cable(Cable::PowerControl(PowerControl::LT(LT {
                        conductor: Conductor::Copper,
                        core_size: "3".to_string(),
                        sqmm: "1.5".to_string(),
                        armoured: true,
                    }))),
                    brand: "kei".to_string(),
                    quantity_mtrs: 50.0,
                    price: 180.50,
                    amount: 9025.00,
                    loading_frls: 0.0,
                    loading_pvc: 0.0,
                },
            ],
            basic_total: 34085.00,
            delivery_charges: 500.00,
            total_with_delivery: 34585.00,
            taxes: 6225.30,
            grand_total: 40810000.30,
            to: Some(
                vec!["Skipper Ltd.", "Kolkata"]
                    .iter()
                    .map(|x| x.to_string())
                    .collect(),
            ),
            terms_and_conditions: Some(
                vec![
                    "Qty. Tolerance: +/-5%",
                    "Payment: Full payment against proforma invoice",
                    "Delivery: Ready stock subject to prior sale",
                    "GST: 18% extra as applicable",
                    "Validity: 3 days from quotation date",
                ]
                .iter()
                .map(|x| x.to_string())
                .collect(),
            ),
        };

        let result = create_quotation_pdf(
            "Q-20250821-TEST",
            "21st August, 2025",
            &test_quotation,
            "test_quotation.pdf",
            DocumentType::Quotation,
        );

        assert!(result.is_ok(), "PDF generation failed: {:?}", result.err());
        assert!(std::path::Path::new("artifacts/test_quotation.pdf").exists());
    }
}
