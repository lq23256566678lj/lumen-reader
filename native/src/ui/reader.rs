use egui::{
    Align, Align2, CentralPanel, ColorImage, FontId, Frame, Label, Layout, Margin, ProgressBar,
    RichText, ScrollArea, Sense, Stroke, TextureHandle, TextureOptions,
};

use crate::app::{LumenApp, PageTextureKey, TocTab};
use crate::state::models::{Bookmark, ViewMode};
use crate::state::reader::{PaginationKey, ReaderSession, paginate_text};
use crate::ui::theme::{self, Palette};
use crate::ui::widgets;

const SIDE_PANEL_WIDTH: f32 = 292.0;

fn push_styled_run(
    job: &mut egui::text::LayoutJob,
    current: &mut String,
    font: &FontId,
    color: egui::Color32,
) {
    if current.is_empty() {
        return;
    }
    job.append(
        current,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color,
            ..Default::default()
        },
    );
    current.clear();
}

impl LumenApp {
    pub(crate) fn show_reader(&mut self, ui: &mut egui::Ui) {
        let Some(mut reader) = self.reader.take() else {
            return;
        };
        self.reader_page_count = reader
            .pagination
            .as_ref()
            .map_or(1, |pagination| pagination.page_count());
        if self.page_error_chapter != Some(reader.chapter) {
            self.page_render_error = None;
            self.page_error_chapter = None;
        }

        let palette = theme::palette(self.settings.theme);
        let mut jump: Option<(usize, f32)> = None;
        let mut delete_bookmark: Option<String> = None;

        if self.settings.toc_open {
            egui::Panel::left("reader_toc")
                .resizable(true)
                .default_size(SIDE_PANEL_WIDTH)
                .min_size(230.0)
                .max_size(390.0)
                .frame(
                    Frame::new()
                        .fill(palette.panel)
                        .inner_margin(Margin::symmetric(12, 12)),
                )
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.toc_tab, TocTab::Contents, "目录");
                        ui.selectable_value(&mut self.toc_tab, TocTab::Bookmarks, "书签");
                    });
                    ui.separator();
                    match self.toc_tab {
                        TocTab::Contents => {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.toc_filter)
                                    .hint_text("筛选目录…")
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(6.0);
                            let filter = self.toc_filter.trim().to_lowercase();
                            let toc = reader.source.toc();
                            let visible: Vec<usize> = if filter.is_empty() {
                                (0..toc.len()).collect()
                            } else {
                                toc.iter()
                                    .enumerate()
                                    .filter(|(_, entry)| {
                                        entry.title.to_lowercase().contains(&filter)
                                    })
                                    .map(|(index, _)| index)
                                    .collect()
                            };

                            // Virtualize the TOC list: large books can have a
                            // thousand entries, and all of them used to be
                            // recreated on every frame.
                            ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show_rows(ui, 25.0, visible.len(), |ui, row_range| {
                                    for row in row_range {
                                        let Some(&entry_index) = visible.get(row) else {
                                            continue;
                                        };
                                        let Some(entry) = toc.get(entry_index) else {
                                            continue;
                                        };
                                        ui.horizontal(|ui| {
                                            ui.add_space((entry.level.min(6) as f32) * 12.0);
                                            let selected = entry.chapter == reader.chapter;
                                            let width = ui.available_width();
                                            let (rect, response) = ui.allocate_exact_size(
                                                egui::vec2(width, 25.0),
                                                Sense::click(),
                                            );
                                            if selected {
                                                ui.painter().rect_filled(
                                                    rect,
                                                    egui::CornerRadius::same(4),
                                                    palette.accent.gamma_multiply(0.14),
                                                );
                                            }
                                            ui.painter().text(
                                                egui::pos2(rect.left() + 7.0, rect.center().y),
                                                Align2::LEFT_CENTER,
                                                widgets::truncate(&entry.title, 36),
                                                FontId::proportional(12.5),
                                                if selected {
                                                    palette.accent
                                                } else {
                                                    palette.text
                                                },
                                            );
                                            if response.clicked() {
                                                jump = Some((entry.chapter, entry.fraction));
                                            }
                                            let _ = response
                                                .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        });
                                    }
                                });
                        }
                        TocTab::Bookmarks => {
                            let bookmarks: Vec<Bookmark> = self
                                .library
                                .active_bookmarks(&reader.book_id)
                                .into_iter()
                                .cloned()
                                .collect();
                            if bookmarks.is_empty() {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new("还没有书签。阅读时点击顶部「书签」或按 B。")
                                        .size(12.0)
                                        .color(palette.muted),
                                );
                            } else {
                                ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for bookmark in bookmarks {
                                            ui.horizontal(|ui| {
                                                let response = ui.add(
                                                    egui::Button::new(
                                                        RichText::new(widgets::truncate(
                                                            &bookmark.label,
                                                            32,
                                                        ))
                                                        .size(12.0),
                                                    )
                                                    .frame(false),
                                                );
                                                if response.clicked() {
                                                    jump =
                                                        Some((bookmark.chapter, bookmark.fraction));
                                                }
                                                if ui
                                                    .small_button("×")
                                                    .on_hover_text("删除书签")
                                                    .clicked()
                                                {
                                                    delete_bookmark = Some(bookmark.id.clone());
                                                }
                                            });
                                        }
                                    });
                            }
                        }
                    }
                });
        }

        // Apply TOC/bookmark navigation before taking the chapter text. If we
        // navigate after rendering, the old chapter text would be restored
        // over the newly loaded chapter.
        if let Some((chapter, fraction)) = jump {
            reader.go_to_toc(chapter, fraction);
        }

        let text = reader.text.take();
        let images = std::mem::take(&mut reader.images);
        CentralPanel::default()
            .frame(Frame::new().fill(palette.bg))
            .show(ui, |ui| {
                ui.add_space(8.0);
                widgets::centered(ui, self.settings.line_width + 80.0, |ui| {
                    self.show_reader_header(ui, &reader, palette);
                });
                ui.add_space(4.0);

                let content_width = self
                    .settings
                    .line_width
                    .min((ui.available_width() - 64.0).max(320.0));
                let body_height = (ui.available_height() - 72.0).max(160.0);

                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_height),
                    Layout::top_down(Align::Center),
                    |ui| {
                        widgets::centered(ui, content_width + 56.0, |ui| {
                            Frame::new()
                                .fill(palette.paper)
                                .stroke(Stroke::new(1.0, palette.border))
                                .corner_radius(egui::CornerRadius::same(10))
                                .inner_margin(Margin::same(28))
                                .show(ui, |ui| {
                                    ui.set_width(content_width);
                                    ui.set_min_height((body_height - 56.0).max(120.0));
                                    if reader.is_paged_format() {
                                        self.show_paged_body(
                                            ui,
                                            &mut reader,
                                            palette,
                                            content_width,
                                            body_height - 56.0,
                                        );
                                    } else {
                                        self.show_text_body(
                                            ui,
                                            &mut reader,
                                            text.as_deref(),
                                            &images,
                                            palette,
                                            content_width,
                                            body_height - 56.0,
                                        );
                                    }
                                });
                        });
                    },
                );

                self.show_reader_footer(ui, &mut reader, palette, content_width);
            });

        reader.text = text;
        reader.images = images;

        if let Some(id) = delete_bookmark {
            self.library.remove_bookmark(&id);
            self.dirty = true;
            self.notify(ui.ctx(), "书签已删除");
        }

        self.reader_page_count = if reader.is_paged_format() {
            reader.chapter_count()
        } else {
            self.reader_page_count.max(1)
        };
        self.reader = Some(reader);
        self.sync_reader_progress();
    }

    fn show_reader_header(&self, ui: &mut egui::Ui, reader: &ReaderSession, palette: Palette) {
        let metadata = reader.source.metadata();
        let record = self.library.book(&reader.book_id);
        let display_title = record
            .map(|book| book.title.trim().to_owned())
            .filter(|title| !title.is_empty())
            .map_or_else(|| metadata.title.clone(), |title| title);
        let display_author = record
            .map(|book| book.author.trim().to_owned())
            .filter(|author| !author.is_empty())
            .map_or_else(|| metadata.author.clone(), |author| author);
        let mut details: Vec<&str> = Vec::new();
        if !metadata.publisher.trim().is_empty() {
            details.push(metadata.publisher.trim());
        }
        if !metadata.language.trim().is_empty() {
            details.push(metadata.language.trim());
        }
        let detail_line = details.join(" · ");
        let hover = if !metadata.description.trim().is_empty() {
            metadata.description.trim().to_owned()
        } else {
            metadata.identifier.trim().to_owned()
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(widgets::truncate(&display_title, 52))
                        .size(20.0)
                        .strong()
                        .color(palette.text),
                )
                .on_hover_text(hover);
                let title_line = format!(
                    "{} · {} · 第 {}/{} 章",
                    if display_author.trim().is_empty() {
                        "未知作者"
                    } else {
                        &display_author
                    },
                    metadata.format.label(),
                    reader.chapter + 1,
                    reader.chapter_count()
                );
                ui.label(
                    RichText::new(if detail_line.is_empty() {
                        title_line
                    } else {
                        format!("{title_line} · {detail_line}")
                    })
                    .size(11.5)
                    .color(palette.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let progress = reader.overall_progress();
                ui.add(
                    ProgressBar::new(progress)
                        .fill(palette.accent)
                        .desired_width(150.0)
                        .text(RichText::new(format!("{:.1}%", progress * 100.0)).size(10.0)),
                );
            });
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(reader.chapter_title())
                .size(14.0)
                .color(palette.accent),
        );
    }

    fn inline_texture(
        &mut self,
        ctx: &egui::Context,
        book_id: &str,
        chapter: usize,
        index: usize,
        bytes: &[u8],
        max_width: f32,
    ) -> Option<TextureHandle> {
        let key = (book_id.to_owned(), chapter, index);
        if let Some(texture) = self.inline_textures.get(&key) {
            return Some(texture.clone());
        }

        let image = image::load_from_memory(bytes).ok()?;
        let target_width = max_width.clamp(240.0, 1000.0).round() as u32;
        let image = if image.width() > target_width {
            let ratio = target_width as f32 / image.width() as f32;
            image.resize_exact(
                target_width,
                ((image.height() as f32 * ratio).round() as u32).max(1),
                image::imageops::FilterType::Triangle,
            )
        } else {
            image
        };
        let rgba = image.to_rgba8();
        let color = ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        let texture = ctx.load_texture(
            format!("inline-{book_id}-{chapter}-{index}"),
            color,
            TextureOptions::LINEAR,
        );
        self.inline_textures.insert(key, texture.clone());
        Some(texture)
    }

    fn add_styled_text(
        &self,
        ui: &mut egui::Ui,
        text: &str,
        font_size: f32,
        default_color: egui::Color32,
    ) {
        let mut job = egui::text::LayoutJob::default();
        let font = FontId::proportional(font_size);
        let chars: Vec<char> = text.chars().collect();
        let mut current = String::new();
        let mut color = default_color;
        let mut index = 0usize;
        while index < chars.len() {
            if chars[index] == '\u{E000}' && index + 7 <= chars.len() {
                push_styled_run(&mut job, &mut current, &font, color);
                let hex: String = chars[index + 1..index + 7].iter().collect();
                if let Ok(value) = u32::from_str_radix(&hex, 16) {
                    color = egui::Color32::from_rgb(
                        ((value >> 16) & 0xFF) as u8,
                        ((value >> 8) & 0xFF) as u8,
                        (value & 0xFF) as u8,
                    );
                }
                index += 7;
            } else if chars[index] == '\u{E001}' {
                push_styled_run(&mut job, &mut current, &font, color);
                color = default_color;
                index += 1;
            } else {
                current.push(chars[index]);
                index += 1;
            }
        }
        push_styled_run(&mut job, &mut current, &font, color);
        ui.add(Label::new(job).wrap());
    }

    fn paint_inline_image(
        &mut self,
        ui: &mut egui::Ui,
        book_id: &str,
        chapter: usize,
        index: usize,
        images: &[Vec<u8>],
        max_width: f32,
    ) {
        let Some(bytes) = images.get(index) else {
            return;
        };
        if self.inline_textures.len() > 64 {
            self.inline_textures.clear();
        }
        let Some(texture) =
            self.inline_texture(ui.ctx(), book_id, chapter, index, bytes, max_width)
        else {
            return;
        };
        let size = texture.size_vec2();
        if size.x <= 0.0 {
            return;
        }
        ui.vertical_centered(|ui| {
            ui.add(
                egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                    .corner_radius(egui::CornerRadius::same(6)),
            );
        });
        ui.add_space(8.0);
    }

    fn add_text_with_images(
        &mut self,
        ui: &mut egui::Ui,
        book_id: &str,
        chapter: usize,
        text: &str,
        images: &[Vec<u8>],
        font_size: f32,
        color: egui::Color32,
    ) {
        use crate::parser::html_text::{IMAGE_END, IMAGE_START};
        let chars: Vec<char> = text.chars().collect();
        let mut index = 0usize;
        let mut run = String::new();
        while index < chars.len() {
            if chars[index] == IMAGE_START {
                self.add_styled_text(ui, &run, font_size, color);
                run.clear();
                let mut end = index + 1;
                while end < chars.len() && chars[end] != IMAGE_END {
                    end += 1;
                }
                if end < chars.len() {
                    let digits: String = chars[index + 1..end].iter().collect();
                    if let Ok(image_index) = digits.parse::<usize>() {
                        self.paint_inline_image(
                            ui,
                            book_id,
                            chapter,
                            image_index,
                            images,
                            ui.available_width(),
                        );
                    }
                    index = end + 1;
                    continue;
                }
            }
            run.push(chars[index]);
            index += 1;
        }
        self.add_styled_text(ui, &run, font_size, color);
    }

    fn show_text_body(
        &mut self,
        ui: &mut egui::Ui,
        reader: &mut ReaderSession,
        text: Option<&str>,
        images: &[Vec<u8>],
        palette: Palette,
        content_width: f32,
        content_height: f32,
    ) {
        if let Some(error) = reader.chapter_error.clone() {
            ui.label(RichText::new(&error).size(13.0).color(palette.danger));
            if ui.button("重试加载本章").clicked() {
                reader.reload_chapter();
            }
            return;
        }
        let Some(text) = text else {
            ui.horizontal(|ui| {
                if reader.is_loading() {
                    ui.spinner();
                }
                ui.label(
                    RichText::new(if reader.is_loading() {
                        "正在加载章节…"
                    } else {
                        "章节暂无内容"
                    })
                    .size(13.0)
                    .color(palette.muted),
                );
            });
            return;
        };

        let font_size = self.settings.font_size;
        match self.settings.view_mode {
            ViewMode::Scroll => {
                let output = ScrollArea::vertical()
                    .id_salt((
                        "reader_scroll",
                        reader.book_id.clone(),
                        reader.chapter,
                        reader.scroll_generation,
                    ))
                    .auto_shrink([false, false])
                    .max_height(content_height)
                    .vertical_scroll_offset(reader.scroll_offset)
                    .show(ui, |ui| {
                        ui.set_width(content_width);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        self.add_text_with_images(
                            ui,
                            &reader.book_id,
                            reader.chapter,
                            text,
                            images,
                            font_size,
                            palette.paper_text,
                        );
                    });
                let max_offset = (output.content_size.y - output.inner_rect.height()).max(0.0);
                if reader.restore_scroll_from_fraction {
                    reader.scroll_restore_attempts =
                        reader.scroll_restore_attempts.saturating_add(1);
                    if max_offset > 1.0 || reader.scroll_restore_attempts >= 2 {
                        if max_offset > 1.0 {
                            reader.scroll_offset = reader.fraction * max_offset;
                        } else {
                            reader.scroll_offset = 0.0;
                            reader.fraction = 0.0;
                            reader.dirty = true;
                        }
                        reader.restore_scroll_from_fraction = false;
                    } else {
                        ui.ctx().request_repaint();
                    }
                } else {
                    reader.update_scroll(
                        output.state.offset.y,
                        output.content_size.y,
                        output.inner_rect.height(),
                    );
                }
            }
            ViewMode::Page => {
                let usable_width = (content_width - 4.0).max(180.0);
                let usable_height = (content_height - 12.0).max(120.0);
                let key = PaginationKey {
                    width: usable_width.round() as u32,
                    height: usable_height.round() as u32,
                    font_size_tenths: (font_size * 10.0).round() as u32,
                };
                let plain_text = crate::parser::html_text::strip_image_markers(text);
                let needs_pagination = reader
                    .pagination
                    .as_ref()
                    .map_or(true, |pagination| pagination.key != key);
                if needs_pagination {
                    let pagination =
                        paginate_text(&plain_text, usable_width, usable_height, font_size);
                    let count = pagination.page_count();
                    reader.pagination = Some(pagination);
                    reader.restored_page = false;
                    reader.restore_page(count);
                }

                let count = reader
                    .pagination
                    .as_ref()
                    .map_or(1, |pagination| pagination.page_count());
                if reader.page_index == usize::MAX {
                    reader.page_index = count.saturating_sub(1);
                }
                let page = reader.page_index.min(count.saturating_sub(1));
                let page_text = reader
                    .pagination
                    .as_ref()
                    .map_or_else(String::new, |pagination| {
                        pagination.page_text(&plain_text, page).to_owned()
                    });
                reader.page_index = page;
                reader.set_page(page, count);
                self.reader_page_count = count;
                ui.ctx().request_repaint();

                ui.add_space(4.0);
                ScrollArea::vertical()
                    .id_salt((
                        "reader_page_scroll",
                        reader.book_id.clone(),
                        reader.chapter,
                        reader.scroll_generation,
                    ))
                    .auto_shrink([false, false])
                    .max_height(content_height)
                    .show(ui, |ui| {
                        ui.set_width(content_width);
                        if page == 0 {
                            for image_index in 0..images.len() {
                                self.paint_inline_image(
                                    ui,
                                    &reader.book_id,
                                    reader.chapter,
                                    image_index,
                                    images,
                                    content_width,
                                );
                            }
                        }
                        self.add_styled_text(ui, &page_text, font_size, palette.paper_text);
                    });
            }
        }
    }

    fn show_paged_body(
        &mut self,
        ui: &mut egui::Ui,
        reader: &mut ReaderSession,
        palette: Palette,
        content_width: f32,
        content_height: f32,
    ) {
        if self.page_error_chapter == Some(reader.chapter) {
            if let Some(error) = self.page_render_error.clone() {
                ui.label(RichText::new(error).size(13.0).color(palette.danger));
                return;
            }
        }

        let page_mode = self.settings.view_mode == ViewMode::Page;
        let display_width = if page_mode {
            content_width.min(content_height * 0.78).max(220.0)
        } else {
            content_width
        };
        let Some((texture, aspect)) = self.render_page_texture(reader, display_width, ui.ctx())
        else {
            let message = self
                .page_render_error
                .clone()
                .map_or_else(|| "正在渲染页面…".to_owned(), |error| error);
            ui.label(RichText::new(message).size(13.0).color(palette.muted));
            return;
        };

        let mut size = egui::vec2(display_width, display_width * aspect);
        if page_mode && size.y > content_height {
            let scale = content_height / size.y;
            size *= scale.max(0.1);
        }

        if page_mode {
            ScrollArea::vertical()
                .id_salt((
                    "paged_page",
                    reader.book_id.clone(),
                    reader.chapter,
                    reader.scroll_generation,
                ))
                .auto_shrink([false, false])
                .max_height(content_height)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                                .corner_radius(egui::CornerRadius::same(4)),
                        );
                    });
                });
            self.reader_page_count = 1;
        } else {
            let output = ScrollArea::vertical()
                .id_salt((
                    "page_scroll",
                    reader.book_id.clone(),
                    reader.chapter,
                    reader.scroll_generation,
                ))
                .auto_shrink([false, false])
                .max_height(content_height)
                .vertical_scroll_offset(reader.scroll_offset)
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::new(egui::load::SizedTexture::new(texture.id(), size))
                                .corner_radius(egui::CornerRadius::same(4)),
                        );
                    });
                });
            let max_offset = (output.content_size.y - output.inner_rect.height()).max(0.0);
            if reader.restore_scroll_from_fraction {
                reader.scroll_restore_attempts = reader.scroll_restore_attempts.saturating_add(1);
                if max_offset > 1.0 || reader.scroll_restore_attempts >= 2 {
                    if max_offset > 1.0 {
                        reader.scroll_offset = reader.fraction * max_offset;
                    } else {
                        reader.scroll_offset = 0.0;
                        reader.fraction = 0.0;
                        reader.dirty = true;
                    }
                    reader.restore_scroll_from_fraction = false;
                } else {
                    ui.ctx().request_repaint();
                }
            } else {
                reader.update_scroll(
                    output.state.offset.y,
                    output.content_size.y,
                    output.inner_rect.height(),
                );
            }
            self.reader_page_count = 1;
        }
    }

    fn render_page_texture(
        &mut self,
        reader: &ReaderSession,
        display_width: f32,
        ctx: &egui::Context,
    ) -> Option<(TextureHandle, f32)> {
        let target_width =
            ((display_width * ctx.pixels_per_point()).round() as u32).clamp(600, 1800);
        let target_width = (target_width / 100).max(1) * 100;
        let key = PageTextureKey {
            book_id: reader.book_id.clone(),
            chapter: reader.chapter,
            width: target_width,
        };
        if let Some(texture) = self.page_textures.get(&key) {
            let aspect = texture.size_vec2().y / texture.size_vec2().x.max(1.0);
            return Some((texture.clone(), aspect));
        }

        match reader.render_page(reader.chapter, target_width) {
            Ok(Some(page)) => {
                let color_image = ColorImage::from_rgba_unmultiplied(
                    [page.width as usize, page.height as usize],
                    &page.rgba,
                );
                let texture = ctx.load_texture(
                    format!(
                        "page-{}-{}-{}",
                        reader.book_id, reader.chapter, target_width
                    ),
                    color_image,
                    TextureOptions::LINEAR,
                );
                let aspect = page.height as f32 / page.width.max(1) as f32;
                if self.page_textures.len() >= 4 {
                    self.page_textures.clear();
                }
                self.page_textures.insert(key, texture.clone());
                self.page_render_error = None;
                self.page_error_chapter = None;
                Some((texture, aspect))
            }
            Ok(None) => None,
            Err(error) => {
                self.page_render_error = Some(error.to_string());
                self.page_error_chapter = Some(reader.chapter);
                None
            }
        }
    }

    fn show_reader_footer(
        &mut self,
        ui: &mut egui::Ui,
        reader: &mut ReaderSession,
        palette: Palette,
        content_width: f32,
    ) {
        ui.add_space(8.0);
        widgets::centered(ui, content_width + 56.0, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(reader.chapter > 0, egui::Button::new("← 上一章"))
                    .clicked()
                {
                    reader.previous_chapter();
                }

                if self.settings.view_mode == ViewMode::Page {
                    if reader.is_paged_format() {
                        if ui
                            .add_enabled(reader.chapter > 0, egui::Button::new("上一页"))
                            .clicked()
                        {
                            reader.previous_chapter();
                        }
                        ui.label(
                            RichText::new(format!(
                                "第 {}/{} 页",
                                reader.chapter + 1,
                                reader.chapter_count().max(1)
                            ))
                            .size(12.0)
                            .color(palette.muted),
                        );
                        if ui
                            .add_enabled(
                                reader.chapter + 1 < reader.chapter_count(),
                                egui::Button::new("下一页 →"),
                            )
                            .clicked()
                        {
                            reader.next_chapter();
                        }
                    } else {
                        let page_count = self.reader_page_count;
                        if ui
                            .add_enabled(
                                reader.chapter > 0 || reader.page_index > 0,
                                egui::Button::new("上一页"),
                            )
                            .clicked()
                        {
                            reader.previous_page(page_count);
                        }
                        ui.label(
                            RichText::new(format!(
                                "第 {}/{} 页",
                                reader.page_index.min(page_count.saturating_sub(1)) + 1,
                                page_count.max(1)
                            ))
                            .size(12.0)
                            .color(palette.muted),
                        );
                        if ui
                            .add_enabled(
                                reader.chapter + 1 < reader.chapter_count()
                                    || reader.page_index.saturating_add(1) < page_count,
                                egui::Button::new("下一页 →"),
                            )
                            .clicked()
                        {
                            reader.next_page(page_count);
                        }
                    }
                } else {
                    ui.label(
                        RichText::new("↑↓ / 滚轮滚动")
                            .size(11.5)
                            .color(palette.muted),
                    );
                }

                if ui
                    .add_enabled(
                        reader.chapter + 1 < reader.chapter_count(),
                        egui::Button::new("下一章 →"),
                    )
                    .clicked()
                {
                    reader.next_chapter();
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let progress = reader.overall_progress();
                    ui.add(
                        ProgressBar::new(progress)
                            .fill(palette.accent)
                            .desired_width(150.0)
                            .text(format!("{:.1}%", progress * 100.0)),
                    );
                });
            });
        });
    }
}
