use egui::{Align2, Color32, Frame, Grid, Margin, Order, RichText, ScrollArea, Stroke, TextEdit};

use crate::app::LumenApp;
use crate::state::models::{ThemeMode, ViewMode};
use crate::ui::theme;
use crate::ui::widgets;

impl LumenApp {
    pub(crate) fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let palette = theme::palette(self.settings.theme);
        let mut open = true;
        egui::Window::new("设置")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new("阅读外观")
                        .size(15.0)
                        .strong()
                        .color(palette.text),
                );
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("主题");
                    if ui
                        .selectable_label(self.settings.theme == ThemeMode::Dark, "深色")
                        .clicked()
                    {
                        self.settings.theme = ThemeMode::Dark;
                        theme::apply(ctx, self.settings.theme);
                        self.last_theme = self.settings.theme;
                        self.dirty = true;
                    }
                    if ui
                        .selectable_label(self.settings.theme == ThemeMode::Light, "浅色")
                        .clicked()
                    {
                        self.settings.theme = ThemeMode::Light;
                        theme::apply(ctx, self.settings.theme);
                        self.last_theme = self.settings.theme;
                        self.dirty = true;
                    }
                });

                ui.add_space(6.0);
                ui.label("正文字号");
                if ui
                    .add(
                        egui::Slider::new(&mut self.settings.font_size, 13.0..=42.0)
                            .suffix(" px")
                            .step_by(0.5),
                    )
                    .changed()
                {
                    self.dirty = true;
                }

                ui.add_space(6.0);
                ui.label("阅读区最大宽度");
                if ui
                    .add(
                        egui::Slider::new(&mut self.settings.line_width, 560.0..=980.0)
                            .suffix(" px")
                            .step_by(10.0),
                    )
                    .changed()
                {
                    self.dirty = true;
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("翻页方式");
                    if ui
                        .selectable_label(self.settings.view_mode == ViewMode::Scroll, "滚动")
                        .clicked()
                    {
                        self.settings.view_mode = ViewMode::Scroll;
                    }
                    if ui
                        .selectable_label(self.settings.view_mode == ViewMode::Page, "翻页")
                        .clicked()
                    {
                        self.settings.view_mode = ViewMode::Page;
                    }
                });

                if self.settings.view_mode == ViewMode::Page {
                    ui.label(
                        RichText::new("翻页模式会按窗口宽度与字号分页，只排版当前章节。")
                            .size(11.5)
                            .color(palette.muted),
                    );
                }

                ui.add_space(10.0);
                ui.checkbox(&mut self.settings.restore_last_book, "启动时恢复上次阅读");
                ui.checkbox(&mut self.settings.toc_open, "显示目录侧栏");

                ui.add_space(14.0);
                ui.label(
                    RichText::new("本地数据")
                        .size(15.0)
                        .strong()
                        .color(palette.text),
                );
                ui.separator();
                ui.label(
                    RichText::new(format!("数据目录：{}", self.paths.data_dir.display()))
                        .size(11.5)
                        .color(palette.muted),
                )
                .on_hover_text(self.paths.data_dir.display().to_string());
                ui.label(
                    RichText::new(format!("书架数据：{}", self.paths.library_file.display()))
                        .size(11.5)
                        .color(palette.muted),
                );
                ui.label(
                    RichText::new(format!(
                        "系统字体：{} · {}",
                        self.font_info.path.display(),
                        self.font_info.family
                    ))
                    .size(11.5)
                    .color(palette.muted),
                )
                .on_hover_text(self.font_info.path.display().to_string());

                ui.add_space(8.0);
                ui.label(
                    RichText::new("所有书籍、进度、书签和设置均使用 serde + JSON 保存在本机。")
                        .size(11.5)
                        .color(palette.muted),
                );
            });
        self.settings_open = open;
    }

    pub(crate) fn show_cloud_window(&mut self, ctx: &egui::Context) {
        if !self.cloud_open {
            return;
        }
        let palette = theme::palette(self.settings.theme);
        let test_result = self.test_bind.read_as_ref().map(|result| match result {
            Ok(message) => Ok(message.clone()),
            Err(error) => Err(error.clone()),
        });
        let syncing = self.sync_bind.is_pending();
        let mut open = true;
        let mut request_test = false;
        let mut request_sync = false;
        let mut save_now = false;
        let mut request_close = false;

        egui::Window::new("WebDAV 云同步")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("只同步阅读进度、书签和书架元数据；不会上传任何书籍文件。")
                        .size(12.0)
                        .color(palette.muted),
                );
                ui.add_space(8.0);
                ui.separator();

                Grid::new("webdav_fields")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("服务器地址");
                        ui.add(
                            TextEdit::singleline(&mut self.cloud.url)
                                .hint_text("https://dav.example.com/remote.php/dav/files/me/")
                                .desired_width(380.0),
                        );
                        ui.end_row();

                        ui.label("用户名");
                        ui.add(
                            TextEdit::singleline(&mut self.cloud.username)
                                .desired_width(380.0),
                        );
                        ui.end_row();

                        ui.label("密码");
                        ui.add(
                            TextEdit::singleline(&mut self.cloud.password)
                                .password(true)
                                .desired_width(380.0),
                        );
                        ui.end_row();

                        ui.label("远端目录");
                        ui.add(
                            TextEdit::singleline(&mut self.cloud.remote_dir)
                                .hint_text("books/lumen（可留空）")
                                .desired_width(380.0),
                        );
                        ui.end_row();
                    });

                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("设备 ID：{}", self.cloud.device_id))
                        .size(11.0)
                        .monospace()
                        .color(palette.muted),
                );
                ui.label(
                    RichText::new(
                        "密码会保存在本地 cloud.json；设置环境变量 LUMEN_WEBDAV_PASSWORD 可避免写入磁盘。",
                    )
                    .size(11.0)
                    .color(palette.muted),
                );
                if let Some(at) = self.cloud.last_sync_at {
                    ui.label(
                        RichText::new(format!(
                            "上次同步：{}（{}）",
                            widgets::relative_time(at),
                            self.cloud
                                .last_sync_status
                                .as_deref()
                                .map_or("", |status| status)
                        ))
                        .size(11.0)
                        .color(palette.muted),
                    );
                }

                if let Some(result) = &test_result {
                    ui.add_space(6.0);
                    match result {
                        Ok(message) => {
                            ui.label(RichText::new(message).size(12.0).color(palette.success));
                        }
                        Err(error) => {
                            ui.label(
                                RichText::new(format!("测试失败：{error}"))
                                    .size(12.0)
                                    .color(palette.danger),
                            );
                        }
                    }
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("保存配置").clicked() {
                        save_now = true;
                    }
                    if ui
                        .add_enabled(
                            !self.test_bind.is_pending(),
                            egui::Button::new(if self.test_bind.is_pending() {
                                "测试中…"
                            } else {
                                "测试连接"
                            }),
                        )
                        .clicked()
                    {
                        request_test = true;
                    }
                    if ui
                        .add_enabled(!syncing, egui::Button::new("立即同步"))
                        .clicked()
                    {
                        request_sync = true;
                    }
                    if syncing {
                        ui.spinner();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("关闭").clicked() {
                            request_close = true;
                        }
                    });
                });

                ui.add_space(4.0);
                ui.label(
                    RichText::new("冲突策略：每本书的进度按 updated_at 最后写入者优先；书签按 ID 合并，删除也会同步。")
                        .size(11.0)
                        .color(palette.muted),
                );
            });

        self.cloud_open = open && !request_close;
        if save_now {
            self.cloud.ensure_device_id();
            self.dirty = true;
            if let Err(error) = self.save_all() {
                self.report_error(ctx, error.to_string());
            } else {
                self.notify(ctx, "WebDAV 配置已保存");
            }
        }
        if request_test {
            self.pending_test_request = true;
        }
        if request_sync {
            self.pending_sync_request = true;
        }
    }

    pub(crate) fn show_shortcuts_window(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        let palette = theme::palette(self.settings.theme);
        let mut open = true;
        egui::Window::new("快捷键")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                Grid::new("shortcut_grid")
                    .num_columns(2)
                    .spacing([20.0, 8.0])
                    .show(ui, |ui| {
                        for (key, action) in [
                            ("Esc", "返回书架 / 关闭窗口"),
                            ("← / →", "上一页 / 下一页"),
                            ("PageUp / PageDown", "上一页 / 下一页"),
                            ("空格", "翻页模式下下一页"),
                            ("+ / −", "放大 / 缩小字号"),
                            ("T", "显示 / 隐藏目录"),
                            ("V", "切换滚动 / 翻页"),
                            ("D", "切换亮暗主题"),
                            ("B", "添加书签"),
                            ("Ctrl+O", "导入电子书"),
                            ("Ctrl+S", "立即云同步"),
                        ] {
                            ui.label(RichText::new(key).monospace().color(palette.accent));
                            ui.label(RichText::new(action).color(palette.text));
                            ui.end_row();
                        }
                    });
                ui.add_space(8.0);
                ui.label(
                    RichText::new("快捷键在文本框未聚焦时生效。")
                        .size(11.0)
                        .color(palette.muted),
                );
            });
        self.shortcuts_open = open;
    }

    pub(crate) fn show_error_window(&mut self, ctx: &egui::Context) {
        let Some(message) = self.error_dialog.clone() else {
            return;
        };
        let palette = theme::palette(self.settings.theme);
        let mut open = true;
        let mut close = false;
        egui::Window::new("提示")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                    ui.label(RichText::new(&message).size(12.5).color(
                        if message.contains("DRM")
                            || message.contains("失败")
                            || message.contains("错误")
                        {
                            palette.danger
                        } else {
                            palette.text
                        },
                    ));
                });
                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("关闭").clicked() {
                        close = true;
                    }
                });
            });
        if close || !open {
            self.error_dialog = None;
        }
    }

    pub(crate) fn show_remove_confirmation(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_remove.clone() else {
            return;
        };
        let Some(book) = self.library.book(&id).cloned() else {
            self.confirm_remove = None;
            return;
        };
        let palette = theme::palette(self.settings.theme);
        let mut open = true;
        let mut remove = false;
        let mut cancel = false;
        egui::Window::new("移出书架")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(format!(
                        "确定将《{}》移出书架吗？\n书籍文件不会被删除。",
                        book.title
                    ))
                    .size(13.0),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("移出书架").color(Color32::WHITE))
                                .fill(palette.danger),
                        )
                        .clicked()
                    {
                        remove = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });

        if remove {
            if self
                .reader
                .as_ref()
                .is_some_and(|reader| reader.book_id == id)
            {
                self.close_reader();
            }
            if let Some(cover_file) = book.cover_file.as_deref() {
                self.paths.remove_cover(cover_file);
            }
            self.library.books.retain(|entry| entry.id != id);
            self.library
                .bookmarks
                .retain(|bookmark| bookmark.book_id != id);
            if self.settings.last_book_id.as_deref() == Some(id.as_str()) {
                self.settings.last_book_id = None;
            }
            self.dirty = true;
            if let Err(error) = self.save_all() {
                self.report_error(ctx, error.to_string());
            }
            self.confirm_remove = None;
        } else if !open || cancel {
            self.confirm_remove = None;
        }
    }

    pub(crate) fn show_toast(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|input| input.time);
        let palette = theme::palette(self.settings.theme);
        let Some(toast) = self.toast.as_mut() else {
            return;
        };
        if toast.until == 0.0 {
            toast.until = now + 3.0;
        }
        if now > toast.until {
            self.toast = None;
            return;
        }
        let text = toast.text.clone();
        egui::Area::new(egui::Id::new("lumen_toast"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -26.0])
            .order(Order::Foreground)
            .show(ctx, |ui| {
                Frame::new()
                    .fill(palette.card)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(egui::CornerRadius::same(10))
                    .inner_margin(Margin::symmetric(18, 10))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(widgets::truncate(&text, 90))
                                .size(12.5)
                                .color(palette.text),
                        );
                    });
            });
    }
}
