//! PDF files through macOS's PDFKit: each page drawn into a picture of any
//! width, a page's text, search, and the text between two points.
//!
//! PDFKit's objects must stay on one thread, so a [`PdfFile`] keeps the
//! document on a thread of its own and talks to it with messages: the
//! window never waits for a page to be drawn.

use std::path::Path;
use std::sync::mpsc;
use std::thread;

use anyhow::{Context as _, Result, anyhow};
use futures::channel::oneshot;
use objc2::AllocAnyThread as _;
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo, CGImageByteOrderInfo,
};
use objc2_foundation::{NSString, NSStringCompareOptions, NSURL};
use objc2_pdf_kit::{PDFDisplayBox, PDFDocument, PDFPage, PDFSelection};

/// A page's size in points, as shown (rotation applied).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

/// A rectangle on a page, in points from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A page drawn into pixels: blue, green, red, alpha, row by row.
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// One place where a search matched.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    pub page: usize,
    /// One rectangle per line the match covers.
    pub rects: Vec<Rect>,
}

/// Text chosen between two points, and where it lies, line by line.
#[derive(Clone, Debug, PartialEq)]
pub struct Selected {
    pub text: String,
    pub rects: Vec<(usize, Rect)>,
}

/// A point on a page, in points from its top left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PagePoint {
    pub page: usize,
    pub x: f32,
    pub y: f32,
}

enum Job {
    Draw(usize, u32, oneshot::Sender<Option<Picture>>),
    Find(String, oneshot::Sender<Vec<Found>>),
    Select(PagePoint, PagePoint, oneshot::Sender<Option<Selected>>),
    Text(usize, oneshot::Sender<String>),
}

/// An open PDF file.
pub struct PdfFile {
    jobs: mpsc::Sender<Job>,
    pages: Vec<PageSize>,
}

impl PdfFile {
    /// Opens a file; fails if PDFKit cannot read it or it is locked.
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.to_path_buf();
        let (opened, ready) = mpsc::channel::<Result<Vec<PageSize>>>();
        let (jobs, queue) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("PDF".to_owned())
            .spawn(move || {
                let document = match open_document(&path) {
                    Ok(document) => document,
                    Err(error) => {
                        let _ = opened.send(Err(error));
                        return;
                    }
                };
                let sizes = (0..page_count(&document))
                    .filter_map(|index| page(&document, index))
                    .map(|page| page_size(&page))
                    .collect();
                let _ = opened.send(Ok(sizes));
                while let Ok(job) = queue.recv() {
                    autoreleasepool(|_| run(&document, job));
                }
            })?;
        let pages = ready.recv().context("the PDF thread stopped")??;
        Ok(Self { jobs, pages })
    }

    pub fn pages(&self) -> &[PageSize] {
        &self.pages
    }

    /// Draws page `index` `width` pixels wide, on white.
    pub fn draw(&self, index: usize, width: u32) -> oneshot::Receiver<Option<Picture>> {
        let (sender, receiver) = oneshot::channel();
        let _ = self.jobs.send(Job::Draw(index, width, sender));
        receiver
    }

    /// Every place where `query` appears, ignoring case and accents.
    pub fn find(&self, query: &str) -> oneshot::Receiver<Vec<Found>> {
        let (sender, receiver) = oneshot::channel();
        let _ = self.jobs.send(Job::Find(query.to_owned(), sender));
        receiver
    }

    /// The text from one point to another, as when dragging over it.
    pub fn select(&self, from: PagePoint, to: PagePoint) -> oneshot::Receiver<Option<Selected>> {
        let (sender, receiver) = oneshot::channel();
        let _ = self.jobs.send(Job::Select(from, to, sender));
        receiver
    }

    /// The whole text of page `index`.
    pub fn text(&self, index: usize) -> oneshot::Receiver<String> {
        let (sender, receiver) = oneshot::channel();
        let _ = self.jobs.send(Job::Text(index, sender));
        receiver
    }
}

fn open_document(path: &Path) -> Result<Retained<PDFDocument>> {
    let path = NSString::from_str(&path.to_string_lossy());
    // SAFETY: plain Foundation and PDFKit calls with valid objects.
    unsafe {
        let url = NSURL::fileURLWithPath(&path);
        let document = PDFDocument::initWithURL(PDFDocument::alloc(), &url)
            .ok_or_else(|| anyhow!("not a PDF file PDFKit can read"))?;
        if document.isLocked() {
            return Err(anyhow!("the PDF is protected by a password"));
        }
        Ok(document)
    }
}

fn page_count(document: &PDFDocument) -> usize {
    // SAFETY: a plain getter.
    unsafe { document.pageCount() }
}

fn page(document: &PDFDocument, index: usize) -> Option<Retained<PDFPage>> {
    // SAFETY: the index is checked against the count.
    unsafe {
        if index >= document.pageCount() {
            return None;
        }
        document.pageAtIndex(index)
    }
}

/// The page's crop box, where it is drawn from, and its rotation.
fn crop_box(page: &PDFPage) -> (CGRect, i64) {
    // SAFETY: plain getters.
    unsafe {
        let bounds = page.boundsForBox(PDFDisplayBox::CropBox);
        (bounds, page.rotation() as i64)
    }
}

fn page_size(page: &PDFPage) -> PageSize {
    let (bounds, rotation) = crop_box(page);
    let (width, height) = (bounds.size.width as f32, bounds.size.height as f32);
    if rotation.rem_euclid(180) == 90 {
        PageSize {
            width: height,
            height: width,
        }
    } else {
        PageSize { width, height }
    }
}

/// From a point on the page as shown (top left origin) to PDFKit's page
/// space (bottom left origin, crop box offset).
fn to_page_space(page: &PDFPage, x: f32, y: f32) -> CGPoint {
    let (bounds, _) = crop_box(page);
    CGPoint::new(
        bounds.origin.x + x as f64,
        bounds.origin.y + bounds.size.height - y as f64,
    )
}

/// From PDFKit's page space back to the page as shown.
fn from_page_space(page: &PDFPage, rect: CGRect) -> Rect {
    let (bounds, _) = crop_box(page);
    Rect {
        x: (rect.origin.x - bounds.origin.x) as f32,
        y: (bounds.origin.y + bounds.size.height - rect.origin.y - rect.size.height) as f32,
        width: rect.size.width as f32,
        height: rect.size.height as f32,
    }
}

fn run(document: &PDFDocument, job: Job) {
    match job {
        Job::Draw(index, width, reply) => {
            let _ = reply.send(page(document, index).and_then(|page| draw(&page, width)));
        }
        Job::Find(query, reply) => {
            let _ = reply.send(find(document, &query));
        }
        Job::Select(from, to, reply) => {
            let _ = reply.send(select(document, from, to));
        }
        Job::Text(index, reply) => {
            let text = page(document, index)
                // SAFETY: a plain getter.
                .and_then(|page| unsafe { page.string() })
                .map(|text| text.to_string())
                .unwrap_or_default();
            let _ = reply.send(text);
        }
    }
}

fn draw(page: &PDFPage, width: u32) -> Option<Picture> {
    let size = page_size(page);
    if width == 0 || size.width <= 0. {
        return None;
    }
    let scale = width as f64 / size.width as f64;
    let height = (size.height as f64 * scale).round().max(1.) as u32;
    let row = width as usize * 4;
    let mut bgra = vec![0u8; row * height as usize];
    let space = CGColorSpace::new_device_rgb()?;
    // Blue, green, red, alpha in memory: what GPUI's pictures hold.
    let info = CGImageAlphaInfo::PremultipliedFirst.0 | CGImageByteOrderInfo::Order32Little.0;
    // SAFETY: the buffer outlives the context, which is dropped here.
    unsafe {
        let context = CGBitmapContextCreate(
            bgra.as_mut_ptr().cast(),
            width as usize,
            height as usize,
            8,
            row,
            Some(&space),
            info,
        )?;
        CGContext::set_rgb_fill_color(Some(&context), 1., 1., 1., 1.);
        CGContext::fill_rect(
            Some(&context),
            CGRect::new(
                CGPoint::new(0., 0.),
                CGSize::new(width as f64, height as f64),
            ),
        );
        CGContext::scale_ctm(Some(&context), scale, scale);
        page.drawWithBox_toContext(PDFDisplayBox::CropBox, &context);
    }
    Some(Picture {
        width,
        height,
        bgra,
    })
}

fn rects_of(document: &PDFDocument, selection: &PDFSelection) -> Vec<(usize, Rect)> {
    let mut rects = Vec::new();
    // SAFETY: plain getters on objects of this document.
    unsafe {
        for line in selection.selectionsByLine() {
            for page in line.pages() {
                let index = document.indexForPage(&page);
                rects.push((index, from_page_space(&page, line.boundsForPage(&page))));
            }
        }
    }
    rects
}

fn find(document: &PDFDocument, query: &str) -> Vec<Found> {
    if query.is_empty() {
        return Vec::new();
    }
    let options = NSStringCompareOptions::CaseInsensitiveSearch
        | NSStringCompareOptions::DiacriticInsensitiveSearch;
    // SAFETY: a synchronous search on this thread's document.
    let selections =
        unsafe { document.findString_withOptions(&NSString::from_str(query), options) };
    selections
        .iter()
        .filter_map(|selection| {
            let rects = rects_of(document, &selection);
            let page = rects.first()?.0;
            Some(Found {
                page,
                rects: rects.into_iter().map(|(_, rect)| rect).collect(),
            })
        })
        .collect()
}

fn select(document: &PDFDocument, from: PagePoint, to: PagePoint) -> Option<Selected> {
    let (start, end) = (page(document, from.page)?, page(document, to.page)?);
    // SAFETY: both pages belong to the document.
    let selection = unsafe {
        document.selectionFromPage_atPoint_toPage_atPoint(
            &start,
            to_page_space(&start, from.x, from.y),
            &end,
            to_page_space(&end, to.x, to.y),
        )?
    };
    // SAFETY: a plain getter.
    let text = unsafe { selection.string() }.map(|text| text.to_string())?;
    if text.is_empty() {
        return None;
    }
    Some(Selected {
        text,
        rects: rects_of(document, &selection),
    })
}

/// A two-page PDF for tests, here and in Keel.
#[doc(hidden)]
pub mod sample {
    use std::path::Path;

    /// A two-page PDF written by hand, with text in Helvetica.
    pub fn write(path: &Path) {
        let pages = [("Ciao Keel", "Prima pagina"), ("Seconda pagina", "Fine")];
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_owned(),
        ];
        for (index, (first, second)) in pages.iter().enumerate() {
            let content = format!(
                "BT /F1 24 Tf 40 140 Td ({first}) Tj ET\nBT /F1 12 Tf 40 60 Td ({second}) Tj ET\n"
            );
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Resources << /Font << /F1 7 0 R >> >> /Contents {} 0 R >>",
                index * 2 + 4
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ));
        }
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
        let mut file = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(file.len());
            file.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
        }
        let xref = file.len();
        file.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for offset in offsets {
            file.push_str(&format!("{offset:010} 00000 n \n"));
        }
        file.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        ));
        std::fs::write(path, file).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use futures::executor::block_on;

    use super::*;

    fn sample() -> (tempfile::TempDir, PdfFile) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("prova.pdf");
        sample::write(&path);
        let pdf = PdfFile::open(&path).unwrap();
        (folder, pdf)
    }

    #[test]
    fn pages_text_and_search() {
        let (_folder, pdf) = sample();
        assert_eq!(
            pdf.pages(),
            [PageSize {
                width: 300.,
                height: 200.
            }; 2]
        );
        assert!(block_on(pdf.text(1)).unwrap().contains("Seconda pagina"));
        let found = block_on(pdf.find("PAGINA")).unwrap();
        assert_eq!(
            found.iter().map(|found| found.page).collect::<Vec<_>>(),
            [0, 1]
        );
        // "Prima pagina" sits low on page 1: 60 points up from the bottom.
        let rect = found[0].rects[0];
        assert!(rect.y > 120. && rect.y < 150., "{rect:?}");
        assert!(block_on(pdf.find("assente")).unwrap().is_empty());
    }

    #[test]
    fn pages_draw_black_text_on_white() {
        let (_folder, pdf) = sample();
        let picture = block_on(pdf.draw(0, 600)).unwrap().unwrap();
        assert_eq!((picture.width, picture.height), (600, 400));
        let pixel = |x: u32, y: u32| {
            let at = (y * picture.width + x) as usize * 4;
            &picture.bgra[at..at + 4]
        };
        // A corner is white and opaque.
        assert_eq!(pixel(2, 2), [255, 255, 255, 255]);
        // Somewhere in "Ciao Keel" (24 points high, baseline 140 up) is dark.
        let dark = (80..130)
            .flat_map(|y| (80..400).map(move |x| (x, y)))
            .any(|(x, y)| pixel(x, y)[0] < 100);
        assert!(dark, "the title is drawn");
    }

    #[test]
    fn dragging_selects_text() {
        let (_folder, pdf) = sample();
        let selected = block_on(pdf.select(
            PagePoint {
                page: 0,
                x: 35.,
                y: 45.,
            },
            PagePoint {
                page: 0,
                x: 250.,
                y: 65.,
            },
        ))
        .unwrap()
        .unwrap();
        assert!(selected.text.contains("Ciao Keel"), "{selected:?}");
        assert!(selected.rects.iter().all(|(page, _)| *page == 0));
    }

    #[test]
    fn something_else_is_refused() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("finto.pdf");
        std::fs::write(&path, "non sono un PDF").unwrap();
        assert!(PdfFile::open(&path).is_err());
    }

    /// Times a real PDF: `KEEL_TEST_PDF=file.pdf cargo test -p pdf --release
    /// timing -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing_a_real_pdf() {
        let path = std::env::var("KEEL_TEST_PDF").expect("KEEL_TEST_PDF");
        let started = std::time::Instant::now();
        let pdf = PdfFile::open(Path::new(&path)).unwrap();
        println!(
            "{} pages opened in {:?}",
            pdf.pages().len(),
            started.elapsed()
        );
        for page in [0, pdf.pages().len() / 2] {
            let started = std::time::Instant::now();
            let picture = block_on(pdf.draw(page, 1600)).unwrap().unwrap();
            println!(
                "page {page} drawn {}×{} in {:?}",
                picture.width,
                picture.height,
                started.elapsed()
            );
        }
        let started = std::time::Instant::now();
        let found = block_on(pdf.find("texture")).unwrap();
        println!(
            "\"texture\" found {} times in {:?}",
            found.len(),
            started.elapsed()
        );
    }
}
