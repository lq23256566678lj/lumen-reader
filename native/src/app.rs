use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align, Frame, Layout, Margin, RichText, TextureHandle};
use egui_async::Bind;
use egui_file_dialog::FileDialog;

use crate::cloud::{self, CloudSnapshot, SyncReport, WebDavConfig};
use crate::error::AppError;
use crate::state::import::{ImportEvent, ImportTask};
use crate::state::models::{BookFormat, LibraryData, Settings, ThemeMode, ViewMode, now_secs};
use crate::state::reader::ReaderSession;
use crate::state::storage::{self, AppPaths};
use crate::ui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibrarySort {
    Smart,
    Recent,
    Title,
    Added,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TocTab {
    Contents,
    Bookmarks,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub until: f64,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct PageTextureKey {
    pub book_id: String,
    pub chapter: usize,
    pub width: u32,
}

struct OpeningBook {
    id: String,
    title: String,
    old_chapter_count: usize,
    receiver: Receiver<Result<ReaderSession, String>>,
}

pub struct LumenApp {
    pub(crate) paths: AppPaths,
    pub(crate) library: LibraryData,
    pub(crate) settings: Settings,
    pub(crate) cloud: WebDavConfig,
    pub(crate) file_dialog: FileDialog,
    pub(crate) font_info: ui::fonts::FontInfo,
    pub(crate) reader: Option<ReaderSession>,
    opening_book: Option<OpeningBook>,
    pub(crate) reader_page_count: usize,
    pub(crate) cover_textures: HashMap<String, TextureHandle>,
    pub(crate) inline_textures: HashMap<(String, usize, usize), TextureHandle>,
    pub(crate) page_textures: HashMap<PageTextureKey, TextureHandle>,
    pub(crate) page_render_error: Option<String>,
    pub(crate) page_error_chapter: Option<usize>,
    pub(crate) import_task: Option<ImportTask>,
    pub(crate) import_errors: Vec<String>,
    pub(crate) sync_bind: Bind<SyncReport, String>,
    pub(crate) test_bind: Bind<String, String>,
    pub(crate) settings_open: bool,
    pub(crate) cloud_open: bool,
    pub(crate) shortcuts_open: bool,
    pub(crate) error_dialog: Option<String>,
    pub(crate) confirm_remove: Option<String>,
    pub(crate) toast: Option<Toast>,
    pub(crate) library_search: String,
    pub(crate) library_sort: LibrarySort,
    pub(crate) current_collection: Option<String>,
    pub(crate) new_folder_open: bool,
    pub(crate) new_folder_name: String,
    pub(crate) rename_folder: Option<(String, String)>,
    pub(crate) delete_folder: Option<String>,
    pub(crate) toc_filter: String,
    pub(crate) toc_tab: TocTab,
    pub(crate) dirty: bool,
    pub(crate) last_save: Instant,
    pub(crate) pending_sync_request: bool,
    pub(crate) pending_test_request: bool,
    pub(crate) last_theme: ThemeMode,
}

impl LumenApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, AppError> {
        let paths = AppPaths::discover()?;
        let library: LibraryData = storage::load_json(&paths.library_file)?;
        let settings: Settings = storage::load_json(&paths.settings_file)?;
        let mut cloud: WebDavConfig = storage::load_json(&paths.cloud_file)?;
        cloud.ensure_device_id();

        let font_info = ui::fonts::install(&cc.egui_ctx)?;
        ui::theme::apply(&cc.egui_ctx, settings.theme);

        let mut file_dialog = FileDialog::new()
            .add_file_filter_extensions("电子书", BookFormat::extensions())
            .default_file_filter("电子书")
            .show_all_files_filter(false)
            .as_modal(true)
            .load_via_thread(true)
            .title("导入电子书");
        if let Some(directory) = dirs::download_dir().or_else(dirs::home_dir) {
            file_dialog = file_dialog.initial_directory(directory);
        }

        let last_book_id = settings.last_book_id.clone();
        let restore = settings.restore_last_book;
        let last_theme = settings.theme;
        let mut app = Self {
            paths,
            library,
            settings,
            cloud,
            file_dialog,
            font_info,
            reader: None,
            opening_book: None,
            reader_page_count: 1,
            cover_textures: HashMap::new(),
            inline_textures: HashMap::new(),
            page_textures: HashMap::new(),
            page_render_error: None,
            page_error_chapter: None,
            import_task: None,
            import_errors: Vec::new(),
            sync_bind: Bind::new(true),
            test_bind: Bind::new(true),
            settings_open: false,
            cloud_open: false,
            shortcuts_open: false,
            error_dialog: None,
            confirm_remove: None,
            toast: None,
            library_search: String::new(),
            library_sort: LibrarySort::Smart,
            current_collection: None,
            new_folder_open: false,
            new_folder_name: String::new(),
            rename_folder: None,
            delete_folder: None,
            toc_filter: String::new(),
            toc_tab: TocTab::Contents,
            dirty: false,
            last_save: Instant::now(),
            pending_sync_request: false,
            pending_test_request: false,
            last_theme,
        };

        if restore {
            if let Some(id) = last_book_id {
                if app
                    .library
                    .book(&id)
                    .is_some_and(|book| book.is_available())
                {
                    app.open_book(&id);
                }
            }
        }
        Ok(app)
    }

    pub fn notify(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        let until = ctx.input(|input| input.time) + 3.2;
        self.toast = Some(Toast {
            text: text.into(),
            until,
        });
        ctx.request_repaint();
    }

    pub fn report_error(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        let text = text.into();
        self.error_dialog = Some(text.clone());
        self.notify(ctx, text);
    }

    pub(crate) fn open_book(&mut self, id: &str) {
        if self.opening_book.is_some() {
            return;
        }
        let Some(record) = self.library.book(id).cloned() else {
            self.reader = None;
            return;
        };
        if !record.is_available() {
            self.toast = Some(Toast {
                text: "这是云端书目。导入相同的书籍文件后即可继续阅读。".to_owned(),
                until: 0.0,
            });
            return;
        }

        let title = record.title.clone();
        let old_chapter_count = record.chapter_count;
        let view = self.settings.view_mode;
        let (sender, receiver) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name(format!("lumen-open-{id}"))
            .spawn(move || {
                let result = ReaderSession::open(&record, view).map_err(|error| error.to_string());
                let _ = sender.send(result);
            });
        if let Err(error) = spawned {
            self.reader = None;
            self.error_dialog = Some(format!("无法启动打开书籍的后台任务：{error}"));
            return;
        }

        self.reader = None;
        self.page_textures.clear();
        self.page_render_error = None;
        self.page_error_chapter = None;
        self.opening_book = Some(OpeningBook {
            id: id.to_owned(),
            title,
            old_chapter_count,
            receiver,
        });
    }

    fn show_opening_overlay(&self, ctx: &egui::Context) {
        let Some(opening) = self.opening_book.as_ref() else {
            return;
        };
        egui::Window::new("正在打开")
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new(format!("正在整理《{}》的章节…", opening.title)).size(13.0),
                    );
                });
            });
    }

    fn poll_opening(&mut self, ctx: &egui::Context) {
        let Some(opening) = self.opening_book.as_ref() else {
            return;
        };
        let received = match opening.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(16));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("打开书籍的后台任务意外退出".to_owned()),
        };
        let Some(opening) = self.opening_book.take() else {
            return;
        };
        match received {
            Ok(session) => self.finish_open(&opening, session),
            Err(error) => {
                self.reader = None;
                self.error_dialog = Some(error);
            }
        }
        ctx.request_repaint();
    }

    fn finish_open(&mut self, opening: &OpeningBook, session: ReaderSession) {
        let normalized_count = session.chapter_count();
        let current_chapter = session.chapter;
        let current_fraction = session.fraction;
        let id = opening.id.as_str();
        if let Some(book) = self.library.book_mut(id) {
            book.last_opened_at = Some(now_secs());
            book.chapter_count = normalized_count;
            book.progress.chapter = current_chapter;
            book.progress.fraction = current_fraction;
        }
        if opening.old_chapter_count > 0 && opening.old_chapter_count != normalized_count {
            let now = now_secs();
            for bookmark in &mut self.library.bookmarks {
                if bookmark.book_id != id {
                    continue;
                }
                let overall = (bookmark.chapter as f32 + bookmark.fraction.clamp(0.0, 1.0))
                    / opening.old_chapter_count as f32;
                let scaled = (overall.clamp(0.0, 1.0) * normalized_count as f32)
                    .clamp(0.0, normalized_count.saturating_sub(1) as f32);
                bookmark.chapter = scaled.floor() as usize;
                bookmark.fraction = scaled.fract();
                bookmark.updated_at = now;
            }
        }
        self.settings.last_book_id = Some(id.to_owned());
        self.reader_page_count = 1;
        self.page_render_error = None;
        self.page_error_chapter = None;
        self.page_textures.clear();
        self.reader = Some(session);
        self.dirty = true;
    }

    pub(crate) fn close_reader(&mut self) {
        self.sync_reader_progress();
        self.reader = None;
        self.page_textures.clear();
        self.reader_page_count = 1;
        self.dirty = true;
        if let Err(error) = self.save_all() {
            self.error_dialog = Some(error.to_string());
        }
    }

    pub(crate) fn sync_reader_progress(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            return;
        };
        if !reader.dirty {
            return;
        }
        let progress = reader.progress();
        let book_id = reader.book_id.clone();
        reader.dirty = false;
        if let Some(book) = self.library.book_mut(&book_id) {
            book.progress = progress;
            book.last_opened_at = Some(now_secs());
        }
        self.dirty = true;
    }

    pub(crate) fn save_all(&mut self) -> Result<(), AppError> {
        self.sync_reader_progress();
        storage::save_json(&self.paths.library_file, &self.library)?;
        storage::save_json(&self.paths.settings_file, &self.settings)?;
        storage::save_json(&self.paths.cloud_file, &self.cloud)?;
        self.dirty = false;
        self.last_save = Instant::now();
        Ok(())
    }

    pub(crate) fn start_import(&mut self, paths: Vec<std::path::PathBuf>) {
        if paths.is_empty() {
            return;
        }
        if self.import_task.is_some() {
            self.toast = Some(Toast {
                text: "已有导入任务正在进行".to_owned(),
                until: 0.0,
            });
            return;
        }
        self.import_errors.clear();
        self.import_task = Some(ImportTask::spawn(paths, self.paths.clone()));
    }

    pub(crate) fn start_sync(&mut self) {
        if !self.cloud.configured() {
            self.cloud_open = true;
            return;
        }
        let config = self.cloud.clone();
        let snapshot = CloudSnapshot::from_library(&self.library, &self.cloud.device_id);
        self.sync_bind.request(async move {
            cloud::sync_now(&config, snapshot)
                .await
                .map_err(|error| error.to_string())
        });
    }

    pub(crate) fn start_test_connection(&mut self) {
        let config = self.cloud.clone();
        self.test_bind.request(async move {
            cloud::test_connection(&config)
                .await
                .map_err(|error| error.to_string())
        });
    }

    pub(crate) fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.settings.theme = self.settings.theme.toggled();
        self.last_theme = self.settings.theme;
        ui::theme::apply(ctx, self.settings.theme);
        self.dirty = true;
    }

    pub(crate) fn adjust_font_size(&mut self, delta: f32) {
        self.settings.font_size = (self.settings.font_size + delta).clamp(13.0, 42.0);
        self.dirty = true;
    }

    pub(crate) fn add_bookmark(&mut self) {
        let Some(reader) = self.reader.as_ref() else {
            return;
        };
        let bookmark = reader.make_bookmark();
        self.library.add_bookmark(bookmark);
        self.dirty = true;
    }

    fn poll_bindings(&mut self, ctx: &egui::Context) {
        self.sync_bind.poll();
        if let Some(result) = self.sync_bind.take() {
            match result {
                Ok(report) => {
                    report.snapshot.apply_to_library(&mut self.library);
                    self.cloud.last_sync_at = Some(report.uploaded_at);
                    self.cloud.last_sync_status = Some(format!(
                        "已同步 {} 本书、{} 个书签",
                        report.books, report.bookmarks
                    ));
                    self.dirty = true;
                    let _ = self.save_all();
                    let direction = if report.remote_existed {
                        "已合并远端数据"
                    } else {
                        "已创建远端快照"
                    };
                    self.notify(
                        ctx,
                        format!(
                            "云同步完成（{direction}）：{} 本书 / {} 个书签",
                            report.books, report.bookmarks
                        ),
                    );
                }
                Err(error) => {
                    self.cloud.last_sync_status = Some(format!("同步失败：{error}"));
                    self.report_error(ctx, format!("云同步失败：{error}"));
                }
            }
        }

        self.test_bind.poll();
        if let Some(result) = self.test_bind.read_as_ref() {
            match result {
                Ok(message) => self.cloud.last_sync_status = Some(message.clone()),
                Err(error) => self.cloud.last_sync_status = Some(format!("连接失败：{error}")),
            }
        }
    }

    fn poll_import(&mut self, ctx: &egui::Context) {
        let mut events = Vec::new();
        if let Some(task) = self.import_task.as_mut() {
            for _ in 0..256 {
                match task.try_recv() {
                    Some(event) => events.push(event),
                    None => break,
                }
            }
        }
        for event in events {
            self.handle_import_event(ctx, event);
        }

        let finished = self
            .import_task
            .as_ref()
            .map_or(false, |task| task.finished);
        if finished {
            self.import_task = None;
            self.dirty = true;
            if let Err(error) = self.save_all() {
                self.error_dialog = Some(error.to_string());
            }
        }
    }

    fn handle_import_event(&mut self, ctx: &egui::Context, event: ImportEvent) {
        match event {
            ImportEvent::Started { total } => {
                self.notify(ctx, format!("开始导入，共发现 {total} 个文件"));
            }
            ImportEvent::Imported(record) => {
                if let Some(existing) = self
                    .library
                    .books
                    .iter_mut()
                    .find(|book| book.id == record.id)
                {
                    existing.path = record.path;
                    existing.file_size = record.file_size;
                    if record.cover_file.is_some() {
                        existing.cover_file = record.cover_file;
                    }
                    if !record.title.trim().is_empty() {
                        existing.title = record.title;
                    }
                    if !record.author.trim().is_empty() {
                        existing.author = record.author;
                    }
                    existing.chapter_count = record.chapter_count;
                    existing.format = record.format;
                } else {
                    self.library.books.push(record);
                }
                self.dirty = true;
            }
            ImportEvent::Failed { path, error } => {
                self.import_errors
                    .push(format!("{}：{error}", short_path(&path)));
                if self.import_errors.len() > 20 {
                    self.import_errors.remove(0);
                }
            }
            ImportEvent::Finished { imported, failed } => {
                let text = format!("导入完成：成功 {imported} 本，失败 {failed} 本");
                self.dirty = true;
                if failed > 0 {
                    self.error_dialog = Some(format!(
                        "{text}\n\n{}",
                        self.import_errors
                            .iter()
                            .rev()
                            .take(8)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n")
                    ));
                }
                self.notify(ctx, text);
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }

        let command = ctx.input(|input| input.modifiers.command);
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        let page_down = ctx.input(|input| input.key_pressed(egui::Key::PageDown));
        let page_up = ctx.input(|input| input.key_pressed(egui::Key::PageUp));
        let right = ctx.input(|input| input.key_pressed(egui::Key::ArrowRight));
        let left = ctx.input(|input| input.key_pressed(egui::Key::ArrowLeft));
        let space = ctx.input(|input| input.key_pressed(egui::Key::Space));
        let plus = ctx.input(|input| {
            input.key_pressed(egui::Key::Plus) || input.key_pressed(egui::Key::Equals)
        });
        let minus = ctx.input(|input| input.key_pressed(egui::Key::Minus));
        let theme_key = ctx.input(|input| input.key_pressed(egui::Key::D));
        let toc_key = ctx.input(|input| input.key_pressed(egui::Key::T));
        let bookmark_key = ctx.input(|input| input.key_pressed(egui::Key::B));
        let view_key = ctx.input(|input| input.key_pressed(egui::Key::V));
        let sync_key = command && ctx.input(|input| input.key_pressed(egui::Key::S));
        let open_key = command && ctx.input(|input| input.key_pressed(egui::Key::O));

        if escape {
            if self.settings_open {
                self.settings_open = false;
            } else if self.cloud_open {
                self.cloud_open = false;
            } else if self.shortcuts_open {
                self.shortcuts_open = false;
            } else if self.reader.is_some() {
                self.close_reader();
            }
        }

        if open_key {
            self.file_dialog.pick_multiple();
        }

        if sync_key {
            if self.cloud.configured() {
                self.start_sync();
            } else {
                self.cloud_open = true;
            }
        }

        if self.reader.is_none() {
            if theme_key {
                self.toggle_theme(ctx);
            }
            return;
        }

        if plus {
            self.adjust_font_size(1.0);
        }
        if minus {
            self.adjust_font_size(-1.0);
        }
        if theme_key {
            self.toggle_theme(ctx);
        }
        if toc_key {
            self.settings.toc_open = !self.settings.toc_open;
        }
        if view_key {
            self.settings.view_mode = self.settings.view_mode.toggled();
            if let Some(reader) = self.reader.as_mut() {
                reader.view = self.settings.view_mode;
                reader.pagination = None;
                reader.restored_page = false;
            }
        }
        if bookmark_key {
            self.add_bookmark();
            self.notify(ctx, "已添加书签");
        }

        let page_mode = self.settings.view_mode == ViewMode::Page;
        let next = page_down || right || (page_mode && space);
        let previous = page_up || left;
        if next || previous {
            let count = self.reader_page_count.max(1);
            if let Some(reader) = self.reader.as_mut() {
                if reader.is_paged_format() {
                    if next {
                        reader.next_chapter();
                    } else {
                        reader.previous_chapter();
                    }
                } else if next {
                    reader.next_page(count);
                } else {
                    reader.previous_page(count);
                }
                self.reader_page_count = count;
            }
        }
    }

    fn show_top_bar(&mut self, ui: &mut egui::Ui) {
        let palette = ui::theme::palette(self.settings.theme);
        let has_reader = self.reader.is_some();
        let mut close_reader = false;
        let mut import_files = false;
        let mut import_folder = false;
        let mut new_folder = false;
        let mut settings_open = false;
        let mut cloud_open = false;
        let mut shortcuts_open = false;
        let mut toggle_theme = false;
        let mut toggle_toc = false;
        let mut toggle_view = false;
        let mut add_bookmark = false;
        let mut font_delta = 0.0_f32;
        let mut start_sync = false;
        let syncing = self.sync_bind.is_pending();

        egui::Panel::top("lumen_top_bar")
            .frame(
                Frame::new()
                    .fill(palette.panel)
                    .inner_margin(Margin::symmetric(18, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("LUMEN")
                            .size(19.0)
                            .strong()
                            .color(palette.accent),
                    );
                    ui.label(RichText::new("微光阅读").size(12.0).color(palette.muted));
                    ui.add_space(10.0);
                    ui.separator();

                    if has_reader {
                        if ui.button("← 书架").clicked() {
                            close_reader = true;
                        }
                        if ui.button("目录").clicked() {
                            toggle_toc = true;
                        }
                        if ui.button(self.settings.view_mode.label()).clicked() {
                            toggle_view = true;
                        }
                        if ui.button("A−").clicked() {
                            font_delta = -1.0;
                        }
                        if ui.button("A+").clicked() {
                            font_delta = 1.0;
                        }
                        if ui.button("书签").clicked() {
                            add_bookmark = true;
                        }
                    } else {
                        if ui.button("导入文件").clicked() {
                            import_files = true;
                        }
                        if ui.button("导入文件夹").clicked() {
                            import_folder = true;
                        }
                        if ui.button("新建文件夹").clicked() {
                            new_folder = true;
                        }
                        ui.label(
                            RichText::new(format!("{} 本本地藏书", self.library.books.len()))
                                .size(12.0)
                                .color(palette.muted),
                        );
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("设置").clicked() {
                            settings_open = true;
                        }
                        if ui.button("云同步").clicked() {
                            cloud_open = true;
                        }
                        if ui
                            .button(match self.settings.theme {
                                ThemeMode::Dark => "浅色",
                                ThemeMode::Light => "深色",
                            })
                            .clicked()
                        {
                            toggle_theme = true;
                        }
                        if ui.button("快捷键").clicked() {
                            shortcuts_open = true;
                        }
                        if syncing {
                            ui.spinner();
                            ui.label(RichText::new("同步中").size(12.0).color(palette.muted));
                        } else if ui
                            .button("立即同步")
                            .on_hover_text("只同步进度、书签和书架元数据，不同步书文件")
                            .clicked()
                        {
                            start_sync = true;
                        }
                    });
                });
            });

        if close_reader {
            self.close_reader();
        }
        if import_files {
            self.file_dialog.pick_multiple();
        }
        if import_folder {
            self.file_dialog.pick_directory();
        }
        if new_folder {
            self.new_folder_name.clear();
            self.new_folder_open = true;
        }
        if settings_open {
            self.settings_open = true;
        }
        if cloud_open {
            self.cloud_open = true;
        }
        if shortcuts_open {
            self.shortcuts_open = true;
        }
        if toggle_theme {
            self.toggle_theme(ui.ctx());
        }
        if toggle_toc {
            self.settings.toc_open = !self.settings.toc_open;
        }
        if toggle_view {
            self.settings.view_mode = self.settings.view_mode.toggled();
            if let Some(reader) = self.reader.as_mut() {
                reader.view = self.settings.view_mode;
                reader.pagination = None;
                reader.restored_page = false;
            }
        }
        if add_bookmark {
            self.add_bookmark();
            self.notify(ui.ctx(), "已添加书签");
        }
        if font_delta != 0.0 {
            self.adjust_font_size(font_delta);
        }
        if start_sync {
            self.start_sync();
        }
    }

    fn update_file_dialog(&mut self, ctx: &egui::Context) {
        self.file_dialog.update(ctx);
        if let Some(paths) = self.file_dialog.take_picked_multiple() {
            self.start_import(paths);
        }
        if let Some(path) = self.file_dialog.take_picked() {
            self.start_import(vec![path]);
        }
    }

    fn auto_save(&mut self) {
        if self.dirty && self.last_save.elapsed() >= Duration::from_millis(900) {
            if let Err(error) = self.save_all() {
                log::error!("autosave failed: {error}");
            }
        }
    }
}

impl eframe::App for LumenApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.plugin_or_default::<egui_async::EguiAsyncPlugin>();
        self.poll_bindings(ctx);
        self.poll_import(ctx);
        self.poll_opening(ctx);
        self.sync_reader_progress();
        if let Some(reader) = self.reader.as_mut() {
            reader.poll_chapter();
            if reader.is_loading() {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }
        self.auto_save();

        if self.import_task.is_some()
            || self.sync_bind.is_pending()
            || self.test_bind.is_pending()
            || self.dirty
        {
            ctx.request_repaint_after(Duration::from_millis(180));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        if self.settings.theme != self.last_theme {
            self.last_theme = self.settings.theme;
            ui::theme::apply(&ctx, self.settings.theme);
        }

        self.show_top_bar(ui);
        if self.reader.is_some() {
            self.show_reader(ui);
        } else {
            self.show_library(ui);
        }

        self.show_settings_window(&ctx);
        self.show_cloud_window(&ctx);
        self.show_shortcuts_window(&ctx);
        self.show_error_window(&ctx);
        self.show_remove_confirmation(&ctx);
        self.show_folder_windows(&ctx);
        self.show_opening_overlay(&ctx);
        self.show_toast(&ctx);
        self.update_file_dialog(&ctx);

        if self.pending_sync_request {
            self.pending_sync_request = false;
            self.start_sync();
        }
        if self.pending_test_request {
            self.pending_test_request = false;
            self.start_test_connection();
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.sync_reader_progress();
        if let Err(error) = self.save_all() {
            log::error!("failed to save on exit: {error}");
        }
    }
}

fn short_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.to_owned(), |name| name.to_owned())
}
