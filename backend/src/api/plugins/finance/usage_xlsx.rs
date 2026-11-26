/*
 * tokensbyte opensource
 * (c) 2026 tokensbyte.ai
 *
 * 「用户 × 模型 × 供应商」明细 xlsx 生成器。
 * 不引入 zip/xlsx 第三方依赖：手搓 OOXML + STORE 打包 + 自算 CRC32。
 * 列顺序、合并单元格、样式与 quota-report skill 的 build_detail_xlsx.py 一一对应，
 * 保证线上导出的文件和本地出的报表是同一张表。
 */

/// 一条明细行（日期由外层分组决定，不进这里）
pub struct XlsxRow {
    pub uid: String,
    pub username: String,
    pub company: String,
    pub model: String,
    pub discount: String,
    pub revenue: f64,
    pub tokens: i128,
    pub supplier: String,
}

pub struct XlsxOut {
    pub bytes: Vec<u8>,
    pub sheets: usize,
    pub rows: usize,
    pub total_revenue: f64,
    pub total_tokens: i128,
}

const COLS: [&str; 9] = ["A", "B", "C", "D", "E", "F", "G", "H", "I"];
const HEADERS: [&str; 5] = ["用户UID", "用户名称", "公司名称", "模型名称", "折扣"];
const METRICS: [&str; 4] = ["收入", "对应成本", "token使用量", "对应供应商"];
const WIDTHS: [i64; 9] = [14, 22, 28, 26, 8, 14, 12, 16, 22];

/// cellXfs 下标，与 skill 里的 9 个 xf 对齐
const ST_HEADER: usize = 1;
const ST_TEXT: usize = 2;
const ST_REVENUE: usize = 3;
const ST_TOKENS: usize = 4;
const ST_DISCOUNT: usize = 5;
const ST_TOTAL_TEXT: usize = 6;
const ST_TOTAL_REVENUE: usize = 7;
const ST_TOTAL_TOKENS: usize = 8;

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

static CRC32_TABLE: [u32; 256] = build_crc_table();

fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in bytes {
        c = CRC32_TABLE[((c ^ (b as u32)) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

/// 时间戳换成 zip 要求的 DOS 格式；1980 之前的年份按 1980 截
pub fn dos_stamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> (u16, u16) {
    let time = ((hour as u16) << 11) | ((minute as u16) << 5) | (second as u16 / 2);
    let date = (((year.max(1980) - 1980) as u16) << 9) | ((month as u16) << 5) | (day as u16);
    (time, date)
}

/// STORE（method 0）打包：xlsx 允许不压缩，省掉一个 deflate 依赖
fn build_zip(files: &[(String, Vec<u8>)], dos_time: u16, dos_date: u16) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();

    for (name, body) in files {
        let nb = name.as_bytes();
        let size = body.len() as u32;
        let sum = crc32(body);
        let offset = out.len() as u32;

        put_u32(&mut out, 0x0403_4b50);
        put_u16(&mut out, 20); // version needed
        put_u16(&mut out, 0); // flags（部件名全是 ASCII）
        put_u16(&mut out, 0); // method = STORE
        put_u16(&mut out, dos_time);
        put_u16(&mut out, dos_date);
        put_u32(&mut out, sum);
        put_u32(&mut out, size);
        put_u32(&mut out, size);
        put_u16(&mut out, nb.len() as u16);
        put_u16(&mut out, 0); // extra len
        out.extend_from_slice(nb);
        out.extend_from_slice(body);

        put_u32(&mut central, 0x0201_4b50);
        put_u16(&mut central, 20); // version made by
        put_u16(&mut central, 20); // version needed
        put_u16(&mut central, 0); // flags
        put_u16(&mut central, 0); // method
        put_u16(&mut central, dos_time);
        put_u16(&mut central, dos_date);
        put_u32(&mut central, sum);
        put_u32(&mut central, size);
        put_u32(&mut central, size);
        put_u16(&mut central, nb.len() as u16);
        put_u16(&mut central, 0); // extra len
        put_u16(&mut central, 0); // comment len
        put_u16(&mut central, 0); // disk number start
        put_u16(&mut central, 0); // internal attrs
        put_u32(&mut central, 0); // external attrs
        put_u32(&mut central, offset);
        central.extend_from_slice(nb);
    }

    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);

    put_u32(&mut out, 0x0605_4b50);
    put_u16(&mut out, 0); // disk
    put_u16(&mut out, 0); // disk with cd
    put_u16(&mut out, files.len() as u16);
    put_u16(&mut out, files.len() as u16);
    put_u32(&mut out, cd_size);
    put_u32(&mut out, cd_offset);
    put_u16(&mut out, 0); // comment len
    out
}

/// 控制字符（含 \x00-\x1f，\t\n\r 之外的一律丢掉）在 XML 1.0 里非法，
/// 公司名称是自由文本，必须过这一关才不会被 Excel 判成坏文件
fn xml_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

fn cell_str(col: &str, rn: usize, text: &str, style: usize) -> String {
    format!(
        "<c r=\"{col}{rn}\" s=\"{style}\" t=\"inlineStr\"><is><t>{}</t></is></c>",
        xml_escape(text)
    )
}

fn cell_num(col: &str, rn: usize, value: &str, style: usize) -> String {
    format!("<c r=\"{col}{rn}\" s=\"{style}\"><v>{value}</v></c>")
}

fn cell_blank(col: &str, rn: usize, style: usize) -> String {
    format!("<c r=\"{col}{rn}\" s=\"{style}\"/>")
}

fn row_xml(rn: usize, cells: &str, height: Option<u32>) -> String {
    match height {
        Some(h) => format!("<row r=\"{rn}\" ht=\"{h}\" customHeight=\"1\">{cells}</row>"),
        None => format!("<row r=\"{rn}\">{cells}</row>"),
    }
}

fn build_sheet_xml(day: &str, rows: &[XlsxRow], first: bool) -> (String, f64, i128) {
    let date_label = day.get(5..).unwrap_or(day);
    let total_row = 3 + rows.len();

    let mut data: Vec<String> = Vec::with_capacity(rows.len() + 3);

    let mut r1 = String::new();
    for (i, h) in HEADERS.iter().enumerate() {
        r1.push_str(&cell_str(COLS[i], 1, h, ST_HEADER));
    }
    r1.push_str(&cell_str(COLS[5], 1, date_label, ST_HEADER));
    data.push(row_xml(1, &r1, Some(22)));

    let mut r2 = String::new();
    for (i, m) in METRICS.iter().enumerate() {
        r2.push_str(&cell_str(COLS[5 + i], 2, m, ST_HEADER));
    }
    data.push(row_xml(2, &r2, Some(22)));

    let mut sum_rev = 0f64;
    let mut sum_tok: i128 = 0;
    for (i, r) in rows.iter().enumerate() {
        let rn = 3 + i;
        let disc: f64 = r.discount.parse().unwrap_or(1.0);
        sum_rev += r.revenue;
        sum_tok += r.tokens;

        let mut cells = String::new();
        cells.push_str(&cell_str(COLS[0], rn, &r.uid, ST_TEXT));
        cells.push_str(&cell_str(COLS[1], rn, &r.username, ST_TEXT));
        cells.push_str(&cell_str(COLS[2], rn, &r.company, ST_TEXT));
        cells.push_str(&cell_str(COLS[3], rn, &r.model, ST_TEXT));
        cells.push_str(&cell_num(COLS[4], rn, &format!("{disc:.2}"), ST_DISCOUNT));
        cells.push_str(&cell_num(COLS[5], rn, &format!("{:.6}", r.revenue), ST_REVENUE));
        cells.push_str(&cell_blank(COLS[6], rn, ST_TEXT));
        cells.push_str(&cell_num(COLS[7], rn, &r.tokens.to_string(), ST_TOKENS));
        cells.push_str(&cell_str(COLS[8], rn, &r.supplier, ST_TEXT));
        data.push(row_xml(rn, &cells, None));
    }

    let mut total = String::new();
    total.push_str(&cell_str(COLS[0], total_row, "合计", ST_TOTAL_TEXT));
    for i in 1..5 {
        total.push_str(&cell_blank(COLS[i], total_row, ST_TOTAL_TEXT));
    }
    total.push_str(&cell_num(COLS[5], total_row, &format!("{:.6}", sum_rev), ST_TOTAL_REVENUE));
    total.push_str(&cell_blank(COLS[6], total_row, ST_TOTAL_TEXT));
    total.push_str(&cell_num(COLS[7], total_row, &sum_tok.to_string(), ST_TOTAL_TOKENS));
    total.push_str(&cell_blank(COLS[8], total_row, ST_TOTAL_TEXT));
    data.push(row_xml(total_row, &total, None));

    let cols: String = WIDTHS
        .iter()
        .enumerate()
        .map(|(i, w)| format!("<col min=\"{}\" max=\"{}\" width=\"{w}\" customWidth=\"1\"/>", i + 1, i + 1))
        .collect();

    let merges = [
        "<mergeCell ref=\"A1:A2\"/>",
        "<mergeCell ref=\"B1:B2\"/>",
        "<mergeCell ref=\"C1:C2\"/>",
        "<mergeCell ref=\"D1:D2\"/>",
        "<mergeCell ref=\"E1:E2\"/>",
        "<mergeCell ref=\"F1:I1\"/>",
    ];
    let tab = if first { " tabSelected=\"1\"" } else { "" };

    let xml = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
            "<sheetViews><sheetView{tab} workbookViewId=\"0\">",
            "<pane ySplit=\"2\" topLeftCell=\"A3\" activePane=\"bottomLeft\" state=\"frozen\"/>",
            "</sheetView></sheetViews>",
            "<sheetFormatPr defaultRowHeight=\"18\"/>",
            "<cols>{cols}</cols>",
            "<sheetData>{body}</sheetData>",
            "<mergeCells count=\"{nmerge}\">{merges}</mergeCells>",
            "</worksheet>"
        ),
        tab = tab,
        cols = cols,
        body = data.concat(),
        nmerge = merges.len(),
        merges = merges.concat(),
    );

    (xml, sum_rev, sum_tok)
}

fn styles_xml() -> String {
    concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">",
        "<numFmts count=\"3\">",
        "<numFmt numFmtId=\"164\" formatCode=\"0.000000\"/>",
        "<numFmt numFmtId=\"165\" formatCode=\"#,##0\"/>",
        "<numFmt numFmtId=\"166\" formatCode=\"0.00\"/>",
        "</numFmts>",
        "<fonts count=\"2\">",
        "<font><sz val=\"11\"/><name val=\"等线\"/><family val=\"2\"/><charset val=\"134\"/></font>",
        "<font><b/><sz val=\"11\"/><name val=\"等线\"/><family val=\"2\"/><charset val=\"134\"/></font>",
        "</fonts>",
        "<fills count=\"3\">",
        "<fill><patternFill patternType=\"none\"/></fill>",
        "<fill><patternFill patternType=\"gray125\"/></fill>",
        "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FFDDEBF7\"/><bgColor indexed=\"64\"/></patternFill></fill>",
        "</fills>",
        "<borders count=\"2\">",
        "<border><left/><right/><top/><bottom/><diagonal/></border>",
        "<border><left style=\"thin\"><color rgb=\"FFBFBFBF\"/></left><right style=\"thin\"><color rgb=\"FFBFBFBF\"/></right>",
        "<top style=\"thin\"><color rgb=\"FFBFBFBF\"/></top><bottom style=\"thin\"><color rgb=\"FFBFBFBF\"/></bottom><diagonal/></border>",
        "</borders>",
        "<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>",
        "<cellXfs count=\"9\">",
        "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>",
        "<xf numFmtId=\"0\" fontId=\"1\" fillId=\"2\" borderId=\"1\" xfId=\"0\" applyFont=\"1\" applyFill=\"1\" applyBorder=\"1\" applyAlignment=\"1\">",
        "<alignment horizontal=\"center\" vertical=\"center\"/></xf>",
        "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyBorder=\"1\" applyAlignment=\"1\">",
        "<alignment vertical=\"center\"/></xf>",
        "<xf numFmtId=\"164\" fontId=\"0\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyNumberFormat=\"1\" applyBorder=\"1\"/>",
        "<xf numFmtId=\"165\" fontId=\"0\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyNumberFormat=\"1\" applyBorder=\"1\"/>",
        "<xf numFmtId=\"166\" fontId=\"0\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyNumberFormat=\"1\" applyBorder=\"1\"/>",
        "<xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyFont=\"1\" applyBorder=\"1\"/>",
        "<xf numFmtId=\"164\" fontId=\"1\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyFont=\"1\" applyNumberFormat=\"1\" applyBorder=\"1\"/>",
        "<xf numFmtId=\"165\" fontId=\"1\" fillId=\"0\" borderId=\"1\" xfId=\"0\" applyFont=\"1\" applyNumberFormat=\"1\" applyBorder=\"1\"/>",
        "</cellXfs>",
        "<cellStyles count=\"1\"><cellStyle name=\"常规\" xfId=\"0\" builtinId=\"0\"/></cellStyles>",
        "</styleSheet>"
    )
    .to_string()
}

/// groups：按自然日分组，组内已按收入降序（沿用 SQL 的排序，不重排）
pub fn build_workbook(
    groups: &[(String, Vec<XlsxRow>)],
    dos_time: u16,
    dos_date: u16,
) -> XlsxOut {
    let mut sheet_xmls: Vec<(String, String)> = Vec::with_capacity(groups.len());
    let mut total_rev = 0f64;
    let mut total_tok: i128 = 0;
    let mut row_count = 0usize;

    for (idx, (day, rows)) in groups.iter().enumerate() {
        let (xml, rev, tok) = build_sheet_xml(day, rows, idx == 0);
        total_rev += rev;
        total_tok += tok;
        row_count += rows.len();
        sheet_xmls.push((format!("xl/worksheets/sheet{}.xml", idx + 1), xml));
    }
    let n = sheet_xmls.len();

    let sheets: String = (1..=n)
        .map(|i| {
            let label = groups[i - 1].0.get(5..).unwrap_or(&groups[i - 1].0);
            format!(
                "<sheet name=\"{}\" sheetId=\"{i}\" r:id=\"rId{i}\"/>",
                xml_escape(label)
            )
        })
        .collect();
    let workbook = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" ",
            "xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">",
            "<sheets>{sheets}</sheets></workbook>"
        ),
        sheets = sheets
    );

    let overrides: String = (1..=n)
        .map(|i| format!(
            "<Override PartName=\"/xl/worksheets/sheet{i}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        ))
        .collect();
    let content_types = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">",
            "<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>",
            "<Default Extension=\"xml\" ContentType=\"application/xml\"/>",
            "<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>",
            "{overrides}",
            "<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>",
            "</Types>"
        ),
        overrides = overrides
    );

    let root_rels = concat!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
        "<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>",
        "</Relationships>"
    );

    let wb_rels: String = (1..=n)
        .map(|i| format!(
            "<Relationship Id=\"rId{i}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{i}.xml\"/>"
        ))
        .collect();
    let workbook_rels = format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
            "{wb_rels}",
            "<Relationship Id=\"rId{styles_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/>",
            "</Relationships>"
        ),
        wb_rels = wb_rels,
        styles_id = n + 1
    );

    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(n + 5);
    files.push(("[Content_Types].xml".to_string(), content_types.into_bytes()));
    files.push(("_rels/.rels".to_string(), root_rels.as_bytes().to_vec()));
    files.push(("xl/workbook.xml".to_string(), workbook.into_bytes()));
    files.push(("xl/_rels/workbook.xml.rels".to_string(), workbook_rels.into_bytes()));
    files.push(("xl/styles.xml".to_string(), styles_xml().into_bytes()));
    for (path, xml) in sheet_xmls {
        files.push((path, xml.into_bytes()));
    }

    XlsxOut {
        bytes: build_zip(&files, dos_time, dos_date),
        sheets: n,
        rows: row_count,
        total_revenue: total_rev,
        total_tokens: total_tok,
    }
}

/// RFC 5987 的 ext-value 编码，中文文件名要它才不丢字
pub fn pct_encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric()
                || b == b'-'
                || b == b'.'
                || b == b'_'
                || b == b'~'
            {
                (b as char).to_string()
            } else {
                format!("%{b:02X}", b = b)
            }
        })
        .collect()
}
