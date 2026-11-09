use egui::{
    Align, Align2, Button, CentralPanel, ColorImage, FontId, Frame, Label, Layout, Margin,
    ProgressBar, RichText, ScrollArea, Sense, Stroke, TextEdit, TextureOptions,
};
use image::GenericImageView;

use crate::app::{LibrarySort, LumenApp};
use crate::state::models::{BookRecord, Collection, ReadingProgress, now_secs};
use crate::state::sorting::smart_title_cmp;
use crate::ui::theme::{self, Palette};
use crate::ui::widgets;

const CARD_WIDTH: f32 = 176.0;
const COVER_WIDTH: f32 = 156.0;
const COVER_HEIGHT: f32 = 228.0;

impl LumenApp {
    pub(crate) fn show_library(&mut self, ui: &mut egui::Ui) {
        let palette = theme::palette(self.settings.theme);
        if let Some(id) = self.current_collection.as_deref() {
            if self.library.collection(id).is_none() {
                self.current_collection = None;
            }
        }
        CentralPanel::default()
            .frame(Frame::new().fill(palette.bg))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(22.0);
                        let content_width = ui.available_width().min(1240.0).max(320.0);
                        widgets::centered(ui, content_width, |ui| {
                            self.show_library_breadcrumb(ui, palette);
                            self.show_library_header(ui, palette);
                            ui.add_space(20.0);
                            self.show_import_progress(ui, palette);
                            if self.current_collection.is_none() {
                                ui.add_space(10.0);
                                self.show_folders(ui, palette, content_width);
                            }
                            ui.add_space(10.0);
                            self.show_book_grid(ui, palette, content_width);
                            ui.add_space(40.0);
                        });
                    });
            });
    }

    fn show_library_breadcrumb(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let Some(id) = self.current_collection.clone() else {
            return;
        };
        let Some(collection) = self.library.collection(&id) else {
            self.current_collection = None;
            return;
        };
        let name = collection.name.clone();
        let count = self.library.collection_book_count(&id);
        let mut back = false;
        let mut rename = false;
        let mut delete = false;

        ui.horizontal(|ui| {
            if ui.button("← 书架").clicked() {
                back = true;
            }
            ui.label(RichText::new("/").size(13.0).color(palette.muted));
            ui.label(RichText::new(&name).size(14.0).strong().color(palette.text));
            ui.label(
                RichText::new(format!("{count} 本"))
                    .size(11.5)
                    .color(palette.muted),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("删除文件夹").clicked() {
                    delete = true;
                }
                if ui.button("重命名").clicked() {
                    rename = true;
                }
            });
        });
        ui.add_space(8.0);

        if back {
            self.current_collection = None;
        }
        if rename {
            self.rename_folder = Some((id.clone(), name));
        }
        if delete {
            self.delete_folder = Some(id);
        }
    }

    fn show_library_header(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let total = self.library.books.len();
        let reading = self
            .library
            .books
            .iter()
            .filter(|book| book.progress.chapter > 0 || book.progress.fraction > 0.001)
            .count();
        let finished = self
            .library
            .books
            .iter()
            .filter(|book| book.overall_progress() >= 0.995)
            .count();
        let last_book = self
            .library
            .books
            .iter()
            .filter(|book| book.last_opened_at.is_some())
            .max_by_key(|book| book.last_opened_at)
            .cloned();
        let mut continue_id = None;
        let folder_title = self
            .current_collection
            .as_deref()
            .and_then(|id| self.library.collection(id))
            .map(|collection| collection.name.clone());
        let (title, subtitle) = if let Some(name) = folder_title {
            let count = self
                .current_collection
                .as_deref()
                .map_or(0, |id| self.library.collection_book_count(id));
            (
                name,
                format!("{count} 本分类书籍 · 右键书籍可移动到其他文件夹"),
            )
        } else {
            (
                "书 架".to_owned(),
                "本地优先的阅读空间。右键书籍可归档到自定义文件夹。".to_owned(),
            )
        };

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(title).size(34.0).strong().color(palette.text));
                ui.label(RichText::new(subtitle).size(13.0).color(palette.muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(last) = &last_book {
                    let label = format!("继续阅读《{}》", widgets::truncate(&last.title, 16));
                    if ui
                        .add_enabled(
                            last.is_available(),
                            Button::new(RichText::new(label).strong())
                                .fill(palette.accent)
                                .corner_radius(egui::CornerRadius::same(8)),
                        )
                        .clicked()
                    {
                        continue_id = Some(last.id.clone());
                    }
                }
            });
        });

        ui.add_space(14.0);
        ui.horizontal(|ui| {
            widgets::stat_card(ui, &total.to_string(), "全部藏书", palette);
            widgets::stat_card(ui, &reading.to_string(), "正在阅读", palette);
            widgets::stat_card(ui, &finished.to_string(), "已读完", palette);
            widgets::stat_card(
                ui,
                &format_size_total(&self.library.books),
                "占用空间",
                palette,
            );
        });

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.library_search)
                    .hint_text("搜索书名或作者…")
                    .desired_width(330.0),
            );
            ui.add_space(8.0);
            egui::ComboBox::from_id_salt("library_sort")
                .selected_text(match self.library_sort {
                    LibrarySort::Smart => "智能排序 · 系列优先",
                    LibrarySort::Recent => "最近阅读",
                    LibrarySort::Title => "按书名（系列优先）",
                    LibrarySort::Added => "按导入时间",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.library_sort,
                        LibrarySort::Smart,
                        "智能排序 · 系列优先",
                    );
                    ui.selectable_value(&mut self.library_sort, LibrarySort::Recent, "最近阅读");
                    ui.selectable_value(
                        &mut self.library_sort,
                        LibrarySort::Title,
                        "按书名（系列优先）",
                    );
                    ui.selectable_value(&mut self.library_sort, LibrarySort::Added, "按导入时间");
                });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new("点击封面打开 · 右键书籍管理或移动到文件夹")
                        .size(11.5)
                        .color(palette.muted),
                );
            });
        });

        if let Some(id) = continue_id {
            self.open_book(&id);
        }
    }

    fn show_import_progress(&mut self, ui: &mut egui::Ui, palette: Palette) {
        let Some(task) = self.import_task.as_ref() else {
            if !self.import_errors.is_empty() {
                egui::CollapsingHeader::new(
                    RichText::new(format!("上次导入有 {} 条问题", self.import_errors.len()))
                        .size(12.0)
                        .color(palette.muted),
                )
                .id_salt("import_errors")
                .show(ui, |ui| {
                    for error in self.import_errors.iter().rev().take(10) {
                        ui.label(RichText::new(error).size(11.5).color(palette.danger));
                    }
                });
            }
            return;
        };

        let fraction = task.fraction();
        Frame::new()
            .fill(palette.card)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(egui::CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("正在导入电子书")
                            .size(13.0)
                            .strong()
                            .color(palette.text),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{}/{} · 成功 {} · 失败 {}",
                            task.done, task.total, task.imported, task.failed
                        ))
                        .size(11.5)
                        .color(palette.muted),
                    );
                });
                ui.add(
                    ProgressBar::new(fraction)
                        .fill(palette.accent)
                        .desired_width(ui.available_width()),
                );
            });
    }

    fn show_folders(&mut self, ui: &mut egui::Ui, palette: Palette, content_width: f32) {
        let folders: Vec<(String, String, usize)> = self
            .library
            .active_collections()
            .into_iter()
            .map(|collection| {
                (
                    collection.id.clone(),
                    collection.name.clone(),
                    self.library.collection_book_count(&collection.id),
                )
            })
            .collect();
        if folders.is_empty() {
            return;
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new("文件夹")
                    .size(14.0)
                    .strong()
                    .color(palette.text),
            );
            if ui.small_button("+ 新建").clicked() {
                self.new_folder_name.clear();
                self.new_folder_open = true;
            }
        });
        ui.add_space(8.0);

        const FOLDER_WIDTH: f32 = 210.0;
        const GAP: f32 = 12.0;
        let columns = (((content_width + GAP) / (FOLDER_WIDTH + GAP)).floor() as usize).max(1);
        for row in folders.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for (id, name, count) in row {
                    self.show_folder_card(ui, id, name, *count, palette);
                }
            });
            ui.add_space(10.0);
        }
        ui.add_space(4.0);
    }

    fn show_folder_card(
        &mut self,
        ui: &mut egui::Ui,
        id: &str,
        name: &str,
        count: usize,
        palette: Palette,
    ) {
        const FOLDER_WIDTH: f32 = 210.0;
        const FOLDER_HEIGHT: f32 = 78.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(FOLDER_WIDTH, FOLDER_HEIGHT), Sense::click());
        let response = response.on_hover_text("点击打开文件夹 · 右键重命名或删除");
        let painter = ui.painter();
        let fill = if response.hovered() {
            palette.card_hover
        } else {
            palette.card
        };
        painter.rect_filled(rect, egui::CornerRadius::same(10), fill);
        painter.rect_stroke(
            rect,
            egui::CornerRadius::same(10),
            Stroke::new(
                if response.hovered() { 1.5 } else { 1.0 },
                if response.hovered() {
                    palette.accent
                } else {
                    palette.border
                },
            ),
            egui::StrokeKind::Inside,
        );
        let left = rect.left() + 14.0;
        painter.text(
            egui::pos2(left, rect.top() + 13.0),
            Align2::LEFT_TOP,
            "文件夹",
            FontId::proportional(10.5),
            palette.muted,
        );
        painter.text(
            egui::pos2(left, rect.top() + 33.0),
            Align2::LEFT_TOP,
            widgets::truncate(name, 15),
            FontId::proportional(15.5),
            palette.text,
        );
        painter.text(
            egui::pos2(left, rect.bottom() - 12.0),
            Align2::LEFT_BOTTOM,
            format!("{count} 本书"),
            FontId::proportional(11.0),
            palette.muted,
        );
        painter.text(
            egui::pos2(rect.right() - 13.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "›",
            FontId::proportional(23.0),
            palette.accent,
        );

        let mut rename = false;
        let mut delete = false;
        response.context_menu(|ui| {
            if ui.button("重命名").clicked() {
                rename = true;
                ui.close();
            }
            if ui.button("删除文件夹").clicked() {
                delete = true;
                ui.close();
            }
        });

        if response.clicked() {
            self.current_collection = Some(id.to_owned());
            self.library_search.clear();
        }
        if rename {
            self.rename_folder = Some((id.to_owned(), name.to_owned()));
        }
        if delete {
            self.delete_folder = Some(id.to_owned());
        }
    }

    fn show_book_grid(&mut self, ui: &mut egui::Ui, palette: Palette, content_width: f32) {
        self.prune_cover_cache();
        let books = self.filtered_books();
        if books.is_empty() {
            show_empty_state(ui, palette, self.library.books.is_empty());
            return;
        }

        // Vertical-only scrolling used to let a long horizontal_wrapped row
        // extend past the right edge, so later books were simply invisible.
        // Compute a real grid from the viewport width and emit one row at a
        // time; all rows live inside the vertical scroll area.
        const GAP: f32 = 14.0;
        const CARD_OUTER_WIDTH: f32 = CARD_WIDTH + 20.0;
        let columns = (((content_width + GAP) / (CARD_OUTER_WIDTH + GAP)).floor() as usize).max(1);

        for row in books.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = GAP;
                for book in row {
                    self.show_book_card(ui, book, palette);
                }
            });
            ui.add_space(16.0);
        }
    }

    fn filtered_books(&self) -> Vec<BookRecord> {
        let query = self.library_search.trim().to_lowercase();
        let collection_filter = self.current_collection.as_deref();
        let mut books: Vec<BookRecord> = self
            .library
            .books
            .iter()
            .filter(|book| {
                if let Some(collection_id) = collection_filter {
                    if book.collection_id.as_deref() != Some(collection_id) {
                        return false;
                    }
                }
                if query.is_empty() {
                    true
                } else {
                    book.title.to_lowercase().contains(&query)
                        || book.author.to_lowercase().contains(&query)
                }
            })
            .cloned()
            .collect();

        match self.library_sort {
            LibrarySort::Smart => {
                books.sort_by(|left, right| smart_title_cmp(&left.title, &right.title));
            }
            LibrarySort::Recent => books.sort_by(|left, right| {
                right
                    .last_opened_at
                    .cmp(&left.last_opened_at)
                    .then_with(|| smart_title_cmp(&left.title, &right.title))
            }),
            LibrarySort::Title => {
                books.sort_by(|left, right| smart_title_cmp(&left.title, &right.title));
            }
            LibrarySort::Added => books.sort_by(|left, right| {
                right
                    .added_at
                    .cmp(&left.added_at)
                    .then_with(|| smart_title_cmp(&left.title, &right.title))
            }),
        }
        books
    }

    fn show_book_card(&mut self, ui: &mut egui::Ui, book: &BookRecord, palette: Palette) {
        let cover = self.cover_texture(book, ui.ctx());
        let folders: Vec<(String, String)> = self
            .library
            .active_collections()
            .into_iter()
            .map(|collection| (collection.id.clone(), collection.name.clone()))
            .collect();
        let mut open = false;
        let mut remove = None;
        let mut reset = false;
        let mut move_to = None;

        Frame::new()
            .fill(palette.card)
            .stroke(Stroke::new(1.0, palette.border))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(CARD_WIDTH);
                ui.vertical(|ui| {
                    let cover_size = egui::vec2(COVER_WIDTH, COVER_HEIGHT);
                    let (cover_rect, response) = ui.allocate_exact_size(cover_size, Sense::click());
                    ui.painter().rect_filled(
                        cover_rect,
                        egui::CornerRadius::same(8),
                        palette.card_hover,
                    );
                    if let Some(texture) = cover {
                        let texture_size = texture.size_vec2();
                        if texture_size.x > 0.0 && texture_size.y > 0.0 {
                            // Aspect-fit the whole cover inside the card instead
                            // of letting egui crop its top-left corner.
                            let scale =
                                (cover_size.x / texture_size.x).min(cover_size.y / texture_size.y);
                            let display_size = texture_size * scale;
                            let image_rect =
                                egui::Rect::from_center_size(cover_rect.center(), display_size);
                            egui::Image::new(egui::load::SizedTexture::new(
                                texture.id(),
                                display_size,
                            ))
                            .corner_radius(egui::CornerRadius::same(8))
                            .paint_at(ui, image_rect);
                        }
                    } else {
                        widgets::paint_cover_placeholder(
                            ui,
                            cover_rect,
                            &book.title,
                            book.format,
                            palette,
                        );
                    }
                    ui.painter().rect_stroke(
                        cover_rect,
                        egui::CornerRadius::same(8),
                        Stroke::new(1.0, palette.border),
                        egui::StrokeKind::Inside,
                    );
                    if response.clicked() {
                        open = true;
                    }
                    response.on_hover_text(if book.is_available() {
                        "打开这本书"
                    } else {
                        "云端书目，导入文件后可阅读"
                    });

                    ui.add_space(8.0);
                    let title = ui.add(
                        Label::new(
                            RichText::new(widgets::truncate(&book.title, 20))
                                .size(13.5)
                                .strong()
                                .color(palette.text),
                        )
                        .truncate()
                        .sense(Sense::click()),
                    );
                    if title.clicked() {
                        open = true;
                    }
                    ui.label(
                        RichText::new(widgets::truncate(book.display_author(), 20))
                            .size(11.5)
                            .color(palette.muted),
                    );

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        widgets::pill(ui, book.format.label(), book.format.color(), palette);
                        if !book.is_available() {
                            widgets::pill(ui, "云端", palette.accent, palette);
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.menu_button("...", |ui| {
                                if ui.button("移出书架").clicked() {
                                    remove = Some(book.id.clone());
                                    ui.close();
                                }
                                if ui.button("标记未读").clicked() {
                                    reset = true;
                                    ui.close();
                                }
                                ui.separator();
                                ui.menu_button("移动到文件夹", |ui| {
                                    if book.collection_id.is_some()
                                        && ui.button("移出文件夹").clicked()
                                    {
                                        move_to = Some(None);
                                        ui.close();
                                    }
                                    if folders.is_empty() {
                                        ui.label(
                                            RichText::new("还没有文件夹")
                                                .size(11.5)
                                                .color(palette.muted),
                                        );
                                    }
                                    for (folder_id, folder_name) in &folders {
                                        if ui.button(folder_name).clicked() {
                                            move_to = Some(Some(folder_id.clone()));
                                            ui.close();
                                        }
                                    }
                                });
                            });
                        });
                    });

                    ui.add_space(4.0);
                    let progress = book.overall_progress();
                    ui.add(
                        ProgressBar::new(progress)
                            .fill(book.format.color())
                            .desired_width(COVER_WIDTH)
                            .text(RichText::new(format!("{:.0}%", progress * 100.0)).size(10.0)),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} 章 · 上次 {}",
                            book.chapter_count,
                            widgets::relative_time(book.last_opened_at.map_or(0, |value| value))
                        ))
                        .size(10.5)
                        .color(palette.muted),
                    );
                });
            });

        if open {
            self.open_book(&book.id);
        }
        if reset {
            if let Some(record) = self.library.book_mut(&book.id) {
                record.progress = ReadingProgress {
                    updated_at: now_secs(),
                    ..Default::default()
                };
            }
            self.dirty = true;
            self.notify(ui.ctx(), "已标记为未读");
        }
        if let Some(target) = move_to {
            if let Some(record) = self.library.book_mut(&book.id) {
                record.collection_id = target;
            }
            self.dirty = true;
            self.notify(ui.ctx(), "已更新所属文件夹");
        }
        if let Some(id) = remove {
            self.confirm_remove = Some(id);
        }
    }

    pub(crate) fn show_folder_windows(&mut self, ctx: &egui::Context) {
        if self.new_folder_open {
            let mut open = true;
            let mut create = false;
            let mut cancel = false;
            egui::Window::new("新建书架文件夹")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("把同一系列、同一题材或待读的书放在一起。");
                    ui.add_space(6.0);
                    let response = ui.add(
                        TextEdit::singleline(&mut self.new_folder_name)
                            .hint_text("例如：科幻 / 历史 / 待读")
                            .desired_width(320.0),
                    );
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        create = true;
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("创建").clicked() {
                            create = true;
                        }
                        if ui.button("取消").clicked() {
                            create = false;
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if create {
                if self.new_folder_name.trim().is_empty() {
                    self.notify(ctx, "请输入文件夹名称");
                } else {
                    self.create_collection();
                    self.notify(ctx, "文件夹已创建");
                }
            } else if !open {
                self.new_folder_open = false;
            }
        }

        if self.rename_folder.is_some() {
            let mut open = true;
            let mut save = false;
            let mut cancel = false;
            egui::Window::new("重命名文件夹")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    if let Some((_, name)) = self.rename_folder.as_mut() {
                        ui.add(
                            TextEdit::singleline(name)
                                .hint_text("文件夹名称")
                                .desired_width(320.0),
                        );
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("保存").clicked() {
                            save = true;
                        }
                        if ui.button("取消").clicked() {
                            save = false;
                            cancel = true;
                        }
                    });
                });
            if cancel {
                open = false;
            }
            if save {
                if let Some((id, name)) = self.rename_folder.clone() {
                    let name = name.trim().to_owned();
                    if name.is_empty() {
                        self.notify(ctx, "文件夹名称不能为空");
                    } else if let Some(collection) = self.library.collection_mut(&id) {
                        collection.name = name;
                        collection.updated_at = now_secs();
                        self.rename_folder = None;
                        self.dirty = true;
                        self.notify(ctx, "文件夹已重命名");
                    }
                }
            } else if !open {
                self.rename_folder = None;
            }
        }

        if let Some(id) = self.delete_folder.clone() {
            let collection = self.library.collection(&id).cloned();
            if let Some(collection) = collection {
                let count = self.library.collection_book_count(&id);
                let mut open = true;
                let mut confirm = false;
                let mut cancel = false;
                egui::Window::new("删除文件夹？")
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.label(format!(
                            "「{}」中有 {} 本书。删除文件夹只会取消归类，不会删除书籍文件。",
                            collection.name, count
                        ));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("删除文件夹").clicked() {
                                confirm = true;
                            }
                            if ui.button("取消").clicked() {
                                confirm = false;
                                cancel = true;
                            }
                        });
                    });
                if cancel {
                    open = false;
                }
                if confirm {
                    let now = now_secs();
                    if let Some(existing) = self
                        .library
                        .collections
                        .iter_mut()
                        .find(|existing| existing.id == id)
                    {
                        existing.deleted = true;
                        existing.updated_at = now;
                    }
                    for book in &mut self.library.books {
                        if book.collection_id.as_deref() == Some(id.as_str()) {
                            book.collection_id = None;
                        }
                    }
                    if self.current_collection.as_deref() == Some(id.as_str()) {
                        self.current_collection = None;
                    }
                    self.delete_folder = None;
                    self.dirty = true;
                    self.notify(ctx, "文件夹已删除，书籍已移出");
                } else if !open {
                    self.delete_folder = None;
                }
            } else {
                self.delete_folder = None;
            }
        }
    }

    fn create_collection(&mut self) {
        let name = self.new_folder_name.trim().to_owned();
        if name.is_empty() {
            return;
        }
        let now = now_secs();
        let id = format!(
            "folder-{now}-{}-{}",
            self.library.collections.len(),
            std::process::id()
        );
        self.library.collections.push(Collection {
            id,
            name,
            created_at: now,
            updated_at: now,
            deleted: false,
        });
        self.new_folder_open = false;
        self.new_folder_name.clear();
        self.dirty = true;
    }

    pub(crate) fn cover_texture(
        &mut self,
        book: &BookRecord,
        ctx: &egui::Context,
    ) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.cover_textures.get(&book.id) {
            return Some(texture.clone());
        }
        let cover_file = book.cover_file.as_deref()?;
        let bytes = std::fs::read(self.paths.cover_path(cover_file)).ok()?;
        let image = image::load_from_memory(&bytes).ok()?;
        let (width, height) = image.dimensions();
        let rgba = image.to_rgba8();
        let color_image =
            ColorImage::from_rgba_unmultiplied([width as usize, height as usize], rgba.as_raw());
        let texture = ctx.load_texture(
            format!("cover-{}", book.id),
            color_image,
            TextureOptions::LINEAR,
        );
        self.cover_textures.insert(book.id.clone(), texture.clone());
        Some(texture)
    }

    fn prune_cover_cache(&mut self) {
        if self.cover_textures.len() > 160 {
            self.cover_textures.clear();
        }
    }
}

fn show_empty_state(ui: &mut egui::Ui, palette: Palette, empty_library: bool) {
    Frame::new()
        .fill(palette.card)
        .stroke(Stroke::new(1.0, palette.border))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(Margin::same(28))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(if empty_library {
                        "书架还是空的"
                    } else {
                        "没有匹配的书籍"
                    })
                    .size(21.0)
                    .strong()
                    .color(palette.text),
                );
                ui.label(
                    RichText::new(if empty_library {
                        "点击左上角「导入文件」或「导入文件夹」，支持 EPUB、MOBI、AZW/AZW3、PDF、FB2、CBZ、TXT。"
                    } else {
                        "换个关键词试试。"
                    })
                    .size(13.0)
                    .color(palette.muted),
                );
                ui.add_space(20.0);
            });
        });
}

fn format_size_total(books: &[BookRecord]) -> String {
    let total: u64 = books.iter().map(|book| book.file_size).sum();
    widgets::format_size(total)
}
