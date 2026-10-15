//! WhatsApp Stories / Status UI: reel bar, full-screen viewer, and status composer.

use std::time::{Duration, Instant};

use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Layout, Margin, Rect, Sense, Stroke, Vec2, pos2,
    vec2,
};

use crate::app::App;
use crate::model::Action;
use crate::stories::{ContactStories, StoryMediaType};
use crate::theme::{self, Icon, Palette};
use crate::ui::widgets;

pub const STORY_COLORS: &[(u32, &str)] = &[
    (0xFF00A884, "Teal"),
    (0xFF128C7E, "Green"),
    (0xFF6B52AE, "Purple"),
    (0xFFB8336A, "Magenta"),
    (0xFF203554, "Navy"),
    (0xFFD85140, "Coral"),
    (0xFFE08B1B, "Amber"),
    (0xFF333D42, "Charcoal"),
];

pub const STORY_FONTS: &[(&str, u32)] = &[("System", 0), ("Script", 2), ("Bold", 6), ("Serif", 8)];

/// State of the active story viewer overlay.
#[derive(Clone, Debug)]
pub struct StoryViewerState {
    pub sender: String,
    pub index: usize,
    pub started_at: Instant,
    pub paused: bool,
    pub download_requested: bool,
    pub hold_started_at: Option<Instant>,
    pub is_holding: bool,
    pub accumulated_elapsed: Duration,
    pub last_tick: Instant,
    pub reply_text: String,
    pub reply_sent: bool,
    pub reply_sent_at: Option<Instant>,
}

impl StoryViewerState {
    pub fn new(sender: String, index: usize) -> Self {
        let now = Instant::now();
        Self {
            sender,
            index,
            started_at: now,
            paused: false,
            download_requested: false,
            hold_started_at: None,
            is_holding: false,
            accumulated_elapsed: Duration::ZERO,
            last_tick: now,
            reply_text: String::new(),
            reply_sent: false,
            reply_sent_at: None,
        }
    }
}

/// Formats a unix timestamp in seconds to a human-readable relative time.
pub(crate) fn format_relative_time(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(timestamp);

    let diff = now.saturating_sub(timestamp);
    if diff < 60 {
        "Just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        "Yesterday".to_string()
    }
}

/// Converts an ARGB u32 (0xAARRGGBB) to egui Color32.
fn argb_to_color32(argb: u32) -> Color32 {
    let r = ((argb >> 16) & 0xFF) as u8;
    let g = ((argb >> 8) & 0xFF) as u8;
    let b = (argb & 0xFF) as u8;
    Color32::from_rgb(r, g, b)
}

/// Draws the horizontal status / story reel above the chat list in the sidebar.
pub fn bar(app: &mut App, ui: &mut egui::Ui) {
    if !app.search.trim().is_empty() || app.locked_folder || app.show_archived {
        return;
    }

    let palette = app.palette;
    let grouped = app.stories.grouped();

    Frame::new()
        .inner_margin(Margin {
            left: 14,
            right: 14,
            top: 2,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::text(
                    ui,
                    crate::i18n::gettext(app.locale, "Status"),
                    theme::bold(13.0),
                    palette.secondary,
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::icon_button(
                        ui,
                        Icon::Plus,
                        14.0,
                        palette.secondary,
                        palette.text,
                        "Add Status",
                    )
                    .clicked()
                    {
                        app.actions.push(Action::OpenPostStory);
                    }
                });
            });

            ui.add_space(4.0);

            egui::ScrollArea::horizontal()
                .id_salt("stories-reel")
                .animated(false)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(10.0, 0.0);

                        // 1. "My Status" card
                        my_status_item(app, ui, &palette);

                        // 2. Contacts with stories
                        for contact in &grouped {
                            if contact.sender == "me"
                                || contact.sender == app.me.as_deref().unwrap_or("")
                            {
                                continue;
                            }
                            contact_story_item(app, ui, &palette, contact);
                        }
                    });
                });

            ui.add_space(6.0);
            ui.separator();
        });
}

fn fit_inside(area: Rect, size: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return area;
    }
    let scale = (area.width() / size.x).min(area.height() / size.y);
    let fitted = size * scale;
    Rect::from_center_size(area.center(), fitted)
}

fn my_status_item(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let item_size = vec2(54.0, 68.0);
    let (rect, response) = ui.allocate_exact_size(item_size, Sense::click());

    let me_str = app.me.clone().unwrap_or_else(|| "me".to_string());
    let my_stories = app
        .stories
        .stories_by_sender
        .get("me")
        .or_else(|| app.stories.stories_by_sender.get(&me_str))
        .filter(|items| !items.is_empty());

    let has_stories = my_stories.is_some();
    let sender_for_viewer = if app.stories.stories_by_sender.contains_key(&me_str) {
        me_str.clone()
    } else {
        "me".to_string()
    };

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        if has_stories {
            let first_unviewed = my_stories
                .and_then(|items| items.iter().position(|s| !s.viewed))
                .unwrap_or(0);
            app.actions.push(Action::OpenStoryViewer {
                sender: sender_for_viewer,
                index: first_unviewed,
            });
        } else {
            app.actions.push(Action::OpenPostStory);
        }
    }

    if has_stories {
        response.on_hover_text(
            "My status\nClick to view your status update\nUse '+' in header to add another",
        );
    } else {
        response.on_hover_text("My status\nClick to add status update");
    }

    let avatar_center = pos2(rect.center().x, rect.top() + 24.0);

    // If user has active stories, draw ring around avatar
    if has_stories {
        ui.painter().circle_stroke(
            avatar_center,
            24.0,
            Stroke::new(2.2, Color32::from_rgb(0, 168, 132)),
        );
    }

    // Draw user avatar or placeholder
    let me = app.me.clone().unwrap_or_default();
    let me_name = app.me_name.clone().unwrap_or_else(|| "You".to_owned());
    let picture = app.avatar(&me);

    ui.scope_builder(
        egui::UiBuilder::new().max_rect(Rect::from_center_size(avatar_center, vec2(42.0, 42.0))),
        |ui| {
            widgets::avatar(ui, palette, &me_name, &me, 42.0, picture.as_deref());
        },
    );

    // Draw '+' badge at bottom-right of avatar
    let badge_center = pos2(avatar_center.x + 15.0, avatar_center.y + 15.0);
    ui.painter().circle_filled(badge_center, 9.0, palette.panel);
    ui.painter()
        .circle_filled(badge_center, 7.5, Color32::from_rgb(0, 168, 132));
    ui.painter().text(
        badge_center + vec2(0.0, -1.0),
        Align2::CENTER_CENTER,
        "+",
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );

    // Label
    let label_rect = Rect::from_min_size(
        pos2(rect.left(), rect.top() + 50.0),
        vec2(rect.width(), 16.0),
    );
    ui.painter().text(
        label_rect.center(),
        Align2::CENTER_CENTER,
        "My status",
        egui::FontId::proportional(11.0),
        palette.text,
    );
}

fn contact_story_item(
    app: &mut App,
    ui: &mut egui::Ui,
    palette: &Palette,
    contact: &ContactStories,
) {
    let item_size = vec2(54.0, 68.0);
    let (rect, response) = ui.allocate_exact_size(item_size, Sense::click());

    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        let first_unviewed = contact.items.iter().position(|s| !s.viewed).unwrap_or(0);
        app.actions.push(Action::OpenStoryViewer {
            sender: contact.sender.clone(),
            index: first_unviewed,
        });
    }

    let tooltip = format!(
        "{}\n{} status update{}",
        contact.sender_name,
        contact.items.len(),
        if contact.items.len() == 1 { "" } else { "s" }
    );
    response.on_hover_text(tooltip);

    let avatar_center = pos2(rect.center().x, rect.top() + 24.0);
    let ring_color = if contact.has_unviewed {
        Color32::from_rgb(0, 168, 132) // Active WhatsApp green
    } else {
        if palette.dark {
            Color32::from_gray(100)
        } else {
            Color32::from_gray(180)
        }
    };

    // Draw story ring around avatar
    ui.painter()
        .circle_stroke(avatar_center, 24.0, Stroke::new(2.2, ring_color));

    // Draw contact avatar
    let picture = app.avatar(&contact.sender);
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(Rect::from_center_size(avatar_center, vec2(42.0, 42.0))),
        |ui| {
            widgets::avatar(
                ui,
                palette,
                &contact.sender_name,
                &contact.sender,
                42.0,
                picture.as_deref(),
            );
        },
    );

    // Short contact first name
    let short_name = contact
        .sender_name
        .split_whitespace()
        .next()
        .unwrap_or(&contact.sender_name);
    let truncated: String = short_name.chars().take(7).collect();
    let display_text = if short_name.chars().count() > 7 {
        format!("{truncated}…")
    } else {
        truncated
    };

    let label_rect = Rect::from_min_size(
        pos2(rect.left(), rect.top() + 50.0),
        vec2(rect.width(), 16.0),
    );
    ui.painter().text(
        label_rect.center(),
        Align2::CENTER_CENTER,
        display_text,
        egui::FontId::proportional(11.0),
        palette.text,
    );
}

fn render_thumbnail(
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    card_rect: Rect,
    id: &str,
    thumb_bytes: &[u8],
) {
    if let Ok(image) = image::load_from_memory(thumb_bytes) {
        let rgba = image.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let pixels = rgba.into_raw();
        let color_img = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let texture = ctx.load_texture(
            format!("story-thumb-{}", id),
            color_img,
            egui::TextureOptions::LINEAR,
        );
        let img_size = vec2(size[0] as f32, size[1] as f32);
        let fitted_rect = fit_inside(card_rect, img_size);
        ui.painter().image(
            texture.id(),
            fitted_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// Full-screen overlay modal for viewing a story with timer progress.
pub fn viewer_show(app: &mut App, ctx: &egui::Context) {
    viewer_show_in(app, ctx, None);
}

/// Views a story with timer progress, optionally constrained to a custom bounds area.
pub fn viewer_show_in(app: &mut App, ctx: &egui::Context, bounds: Option<Rect>) {
    let Some(state) = app.story_viewer.clone() else {
        return;
    };

    let Some(items) = app.stories.stories_by_sender.get(&state.sender).cloned() else {
        app.actions.push(Action::CloseStoryViewer);
        return;
    };

    if items.is_empty() || state.index >= items.len() {
        app.actions.push(Action::CloseStoryViewer);
        return;
    }

    let mut current_item = items[state.index].clone();

    // Mark current item as viewed if not already
    if !current_item.viewed {
        app.stories.mark_viewed(&state.sender, &current_item.id);
        app.backend.send(crate::backend::Command::ViewStory {
            sender: state.sender.clone(),
            id: current_item.id.clone(),
        });
        let stories_file = app.dirs.state.join("stories.json");
        app.stories.save(&stories_file);
    }

    let is_video = current_item.media_type == Some(StoryMediaType::Video);
    let is_image = current_item.media_type == Some(StoryMediaType::Image);

    // Auto-detect if media file is already present in cache / download directory
    if current_item.media_path.is_none() && (is_video || is_image) {
        let ext = if is_video { "mp4" } else { "jpg" };
        let candidate = app
            .dirs
            .media_cache_dir()
            .join(format!("story-{}.{}", current_item.id, ext));
        if candidate.exists() {
            let path_str = candidate.to_string_lossy().to_string();
            current_item.media_path = Some(path_str.clone());
            app.stories.set_media_path(&current_item.id, path_str);
        }
    }

    // Request on-demand media download if media_path is not yet downloaded
    if current_item.media_path.is_none() && !state.download_requested {
        if let Some(viewer) = app.story_viewer.as_mut() {
            viewer.download_requested = true;
        }
        if let Some(ref raw) = current_item.raw_message {
            app.backend
                .send(crate::backend::Command::DownloadStoryMedia {
                    id: current_item.id.clone(),
                    raw_message: raw.clone(),
                });
        }
    }

    // Auto-clear reply sent toast feedback after 2.5s
    if state.reply_sent {
        if let Some(sent_at) = state.reply_sent_at {
            if sent_at.elapsed() >= Duration::from_secs_f32(2.5) {
                if let Some(viewer) = app.story_viewer.as_mut() {
                    viewer.reply_sent = false;
                    viewer.reply_sent_at = None;
                }
            }
        }
    }

    // Keyboard navigation: disabled while typing a reply
    let typing_reply = !state.reply_text.is_empty() || ctx.egui_wants_keyboard_input();
    let effective_paused = state.paused || state.is_holding || typing_reply;

    let (esc, left, right, space) = ctx.input(|i| {
        (
            i.key_pressed(egui::Key::Escape),
            !typing_reply && i.key_pressed(egui::Key::ArrowLeft),
            !typing_reply
                && (i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::Enter)),
            !typing_reply && i.key_pressed(egui::Key::Space),
        )
    });

    if esc {
        app.actions.push(Action::CloseStoryViewer);
        return;
    }
    if left {
        app.actions.push(Action::PrevStory);
        return;
    }
    if right {
        app.actions.push(Action::NextStory);
        return;
    }
    if space {
        if let Some(viewer) = app.story_viewer.as_mut() {
            viewer.paused = !viewer.paused;
        }
    }

    let is_unsupported_video = is_video && app.video.is_unsupported(&current_item.id);
    let mut video_status = None;

    if is_video && !is_unsupported_video {
        if let Some(ref media_path) = current_item.media_path {
            let path = std::path::Path::new(media_path);
            if path.exists() {
                if app.video.message() != Some(&current_item.id) {
                    app.video.toggle(&current_item.id, path);
                }
                app.video.saw(&current_item.id);
                if effective_paused {
                    app.video.pause();
                } else {
                    app.video.resume();
                }
                video_status = app.video.status(&current_item.id);
            }
        }
    }

    let is_video_loading = is_video
        && !is_unsupported_video
        && (video_status.as_ref().map_or(true, |vs| {
            vs.state == crate::video::State::Loading || vs.frame.is_none()
        }));
    let is_image_loading =
        is_image && current_item.media_path.is_none() && current_item.raw_message.is_some();
    let is_media_loading = is_video_loading || is_image_loading;

    let screen = bounds.unwrap_or_else(|| ctx.content_rect());
    let palette = app.palette;

    let screen_w = screen.width();
    let screen_h = screen.height();

    // Determine intrinsic media aspect ratio (width / height)
    let detected_aspect: Option<f32> = if is_video {
        // 1. Decoded video frame
        let frame_aspect = video_status
            .as_ref()
            .and_then(|vs| vs.frame.as_ref())
            .and_then(|frame| {
                let sz = frame.size_vec2();
                if sz.x > 0.0 && sz.y > 0.0 {
                    Some(sz.x / sz.y)
                } else {
                    None
                }
            });

        // 2. Video thumbnail if frame not yet ready
        frame_aspect.or_else(|| {
            current_item.thumbnail.as_ref().and_then(|thumb_bytes| {
                image::load_from_memory(thumb_bytes).ok().and_then(|img| {
                    let (w, h) = (img.width() as f32, img.height() as f32);
                    if w > 0.0 && h > 0.0 {
                        Some(w / h)
                    } else {
                        None
                    }
                })
            })
        })
    } else if is_image {
        // 1. Image file header
        let file_aspect = current_item.media_path.as_ref().and_then(|path_str| {
            let path = std::path::Path::new(path_str);
            if path.exists() {
                image::image_dimensions(path).ok().and_then(|(w, h)| {
                    if w > 0 && h > 0 {
                        Some(w as f32 / h as f32)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        });

        // 2. Image thumbnail
        file_aspect.or_else(|| {
            current_item.thumbnail.as_ref().and_then(|thumb_bytes| {
                image::load_from_memory(thumb_bytes).ok().and_then(|img| {
                    let (w, h) = (img.width() as f32, img.height() as f32);
                    if w > 0.0 && h > 0.0 {
                        Some(w / h)
                    } else {
                        None
                    }
                })
            })
        })
    } else {
        None
    };

    // Responsive story card sizing based on screen space and content aspect ratio.
    // Allows landscape and square media to be spacious, crisp, and prominent on desktop.
    let pad_x = if screen_w >= 850.0 { 64.0 } else { 20.0 };
    let max_card_w = (screen_w - pad_x * 2.0).clamp(340.0, 1150.0);
    let max_card_h = (screen_h - 48.0).clamp(380.0, 960.0);

    let (target_w, target_h) = match detected_aspect {
        Some(aspect) if aspect >= 1.05 => {
            // Landscape media (e.g. 16:9, 4:3, 3:2):
            let aspect = aspect.clamp(0.4, 2.5);
            let by_height_w = max_card_h * aspect;
            if by_height_w <= max_card_w {
                (by_height_w, max_card_h)
            } else {
                (max_card_w, max_card_w / aspect)
            }
        }
        Some(aspect) if aspect >= 0.95 => {
            // Square media (around 1:1):
            let side = max_card_h.min(max_card_w).min(750.0);
            (side, side)
        }
        Some(aspect) => {
            // Portrait media (e.g. 9:16, 4:5, 3:4):
            let aspect = aspect.clamp(0.4, 2.5);
            let h = max_card_h.min(940.0);
            let w = (h * aspect).min(max_card_w);
            if w < 360.0 && max_card_w >= 360.0 {
                (360.0, (360.0 / aspect).min(max_card_h))
            } else {
                (w, h)
            }
        }
        None => {
            // Text status: well-proportioned reading card
            let h = max_card_h.min(840.0);
            let w = (h * 0.60).clamp(380.0, 520.0).min(max_card_w);
            let final_h = (w / 0.60).min(max_card_h);
            (w, final_h)
        }
    };

    let card_width = target_w.clamp(340.0, max_card_w).min(screen_w - 24.0);
    let card_height = target_h.clamp(380.0, max_card_h).min(screen_h - 24.0);

    let card_rect = Rect::from_center_size(screen.center(), vec2(card_width, card_height));
    let close_rect = Rect::from_center_size(
        pos2(card_rect.right() - 28.0, card_rect.top() + 42.0),
        vec2(32.0, 32.0),
    );
    let unsupported_btn_rect = if is_unsupported_video {
        Rect::from_center_size(
            pos2(card_rect.center().x, card_rect.center().y + 24.0),
            vec2(220.0, 36.0),
        )
    } else {
        Rect::NOTHING
    };

    let show_arrows_outside = screen.width() > card_rect.width() + 90.0;
    let (left_arrow_center, right_arrow_center) = if show_arrows_outside {
        (
            pos2(card_rect.left() - 38.0, card_rect.center().y),
            pos2(card_rect.right() + 38.0, card_rect.center().y),
        )
    } else {
        (
            pos2(card_rect.left() + 28.0, card_rect.center().y),
            pos2(card_rect.right() - 28.0, card_rect.center().y),
        )
    };
    let left_arrow_rect = Rect::from_center_size(left_arrow_center, vec2(44.0, 44.0));
    let right_arrow_rect = Rect::from_center_size(right_arrow_center, vec2(44.0, 44.0));

    let now = Instant::now();
    let pointer_pos = ctx.input(|i| i.pointer.interact_pos());
    let pointer_down = ctx.input(|i| i.pointer.primary_down());
    let pointer_released = ctx.input(|i| i.pointer.primary_released());

    // Press and hold (tap and hold) handling
    if pointer_down {
        if let Some(pos) = pointer_pos {
            if card_rect.contains(pos)
                && !close_rect.contains(pos)
                && !unsupported_btn_rect.contains(pos)
                && !left_arrow_rect.contains(pos)
                && !right_arrow_rect.contains(pos)
                && pos.y <= card_rect.bottom() - 110.0
            {
                if state.hold_started_at.is_none() {
                    if let Some(viewer) = app.story_viewer.as_mut() {
                        viewer.hold_started_at = Some(now);
                    }
                } else if let Some(started) = state.hold_started_at {
                    if now.duration_since(started) >= Duration::from_millis(180)
                        && !state.is_holding
                    {
                        if let Some(viewer) = app.story_viewer.as_mut() {
                            viewer.is_holding = true;
                        }
                    }
                }
            }
        }
    } else if pointer_released || (!pointer_down && state.hold_started_at.is_some()) {
        if let Some(viewer) = app.story_viewer.as_mut() {
            let was_holding = viewer.is_holding;
            let hold_dur = viewer
                .hold_started_at
                .map(|t| t.elapsed())
                .unwrap_or_default();
            viewer.hold_started_at = None;
            viewer.is_holding = false;

            if !was_holding && hold_dur < Duration::from_millis(180) {
                // Quick tap inside content area
                if let Some(pos) = pointer_pos {
                    if card_rect.contains(pos)
                        && !close_rect.contains(pos)
                        && !unsupported_btn_rect.contains(pos)
                        && !left_arrow_rect.contains(pos)
                        && !right_arrow_rect.contains(pos)
                        && pos.y >= card_rect.top() + 70.0
                        && pos.y <= card_rect.bottom() - 110.0
                    {
                        let rel_x = (pos.x - card_rect.left()) / card_rect.width();
                        if rel_x < 0.25 {
                            app.actions.push(Action::PrevStory);
                        } else if rel_x > 0.75 {
                            app.actions.push(Action::NextStory);
                        } else {
                            viewer.paused = !viewer.paused;
                        }
                    }
                }
            }
        }
    }

    // Delta time accumulation for image/text countdown
    let delta = now.saturating_duration_since(state.last_tick);
    if let Some(viewer) = app.story_viewer.as_mut() {
        viewer.last_tick = now;
        if !effective_paused && !is_media_loading {
            viewer.accumulated_elapsed += delta;
        }
    }

    // Progress determination
    let progress = if is_video && !is_unsupported_video {
        if let Some(ref vs) = video_status {
            if is_video_loading {
                ctx.request_repaint_after(Duration::from_millis(50));
                0.0
            } else {
                let frac = vs.fraction();
                if !effective_paused
                    && vs.total > Duration::ZERO
                    && vs.position >= vs.total.saturating_sub(Duration::from_millis(150))
                {
                    app.actions.push(Action::NextStory);
                } else if !effective_paused {
                    ctx.request_repaint_after(Duration::from_millis(25));
                }
                frac
            }
        } else {
            ctx.request_repaint_after(Duration::from_millis(50));
            0.0
        }
    } else if is_image_loading {
        ctx.request_repaint_after(Duration::from_millis(50));
        0.0
    } else {
        let story_duration = 5.0_f32;
        let p = (state.accumulated_elapsed.as_secs_f32() / story_duration).clamp(0.0, 1.0);
        if !effective_paused && p >= 1.0 {
            app.actions.push(Action::NextStory);
        } else if !effective_paused {
            ctx.request_repaint_after(Duration::from_millis(25));
        }
        p
    };

    // Full screen overlay area
    let is_open = app.story_viewer.is_some();
    let t = ctx.animate_bool_with_time(egui::Id::new("story-viewer-surface-anim"), is_open, 0.18);
    egui::Area::new(egui::Id::new("story-viewer-surface"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            ui.set_opacity(t);
            ui.set_min_size(screen.size());

            // Dark backdrop
            ui.painter()
                .rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha((240.0 * t) as u8));

            // Background of the story
            let bg_color = match &current_item.media_type {
                Some(_) => Color32::from_rgb(12, 14, 18),
                None => current_item
                    .background_argb
                    .map(argb_to_color32)
                    .unwrap_or_else(|| Color32::from_rgb(0, 168, 132)),
            };

            ui.painter()
                .rect_filled(card_rect, CornerRadius::same(16), bg_color);

            // Clip painter to card_rect with rounded corners
            ui.set_clip_rect(card_rect);

            // 1. Render Center Media or Text Content
            if let Some(text) = &current_item.text {
                let content_rect = card_rect.shrink2(vec2(32.0, 80.0));
                let font_size = if text.len() < 60 {
                    32.0
                } else if text.len() < 140 {
                    25.0
                } else {
                    20.0
                };
                let font_id = egui::FontId::proportional(font_size);
                let galley = ui.painter().layout(
                    text.clone(),
                    font_id,
                    Color32::WHITE,
                    content_rect.width(),
                );
                let text_center = content_rect.center();
                let text_pos = pos2(
                    text_center.x - galley.size().x / 2.0,
                    text_center.y - galley.size().y / 2.0,
                );
                ui.painter().galley(text_pos, galley, Color32::WHITE);
            } else if is_video {
                // Video story: native playback via app.video
                let mut rendered = false;
                if let Some(ref vs) = video_status {
                    if let Some(ref frame) = vs.frame {
                        let fitted_rect = fit_inside(card_rect, frame.size_vec2());
                        ui.painter().image(
                            frame.id(),
                            fitted_rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        rendered = true;
                    }
                }
                if !rendered {
                    // Try thumbnail while video decoder warms up or video downloads
                    if let Some(thumb_bytes) = &current_item.thumbnail {
                        render_thumbnail(ctx, ui, card_rect, &current_item.id, thumb_bytes);
                    }
                    if is_unsupported_video {
                        let box_rect = Rect::from_center_size(
                            pos2(card_rect.center().x, card_rect.center().y),
                            vec2(300.0, 110.0),
                        );
                        ui.painter().rect_filled(
                            box_rect,
                            CornerRadius::same(12),
                            Color32::from_black_alpha(210),
                        );
                        ui.painter().text(
                            pos2(box_rect.center().x, box_rect.top() + 20.0),
                            Align2::CENTER_CENTER,
                            "Format video tidak didukung pemutar internal",
                            egui::FontId::proportional(12.5),
                            Color32::WHITE,
                        );
                        ui.painter().text(
                            pos2(box_rect.center().x, box_rect.top() + 38.0),
                            Align2::CENTER_CENTER,
                            "(misal: HEVC / H.265)",
                            egui::FontId::proportional(11.0),
                            Color32::from_rgb(180, 180, 180),
                        );

                        if let Some(ref media_path) = current_item.media_path {
                            let btn_resp = ui.allocate_rect(unsupported_btn_rect, Sense::click());
                            let btn_hovered = btn_resp.hovered();
                            let btn_bg = if btn_hovered {
                                palette.accent
                            } else {
                                palette.accent.gamma_multiply(0.85)
                            };
                            ui.painter().rect_filled(
                                unsupported_btn_rect,
                                CornerRadius::same(8),
                                btn_bg,
                            );
                            ui.painter().text(
                                unsupported_btn_rect.center(),
                                Align2::CENTER_CENTER,
                                "▶  Buka di Pemutar Eksternal",
                                egui::FontId::proportional(12.5),
                                Color32::WHITE,
                            );
                            if btn_hovered {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }
                            if btn_resp.clicked() {
                                app.actions
                                    .push(Action::OpenFile(std::path::PathBuf::from(media_path)));
                            }
                        }
                    } else if is_video_loading
                        && (current_item.raw_message.is_some() || current_item.media_path.is_some())
                    {
                        let disc = Rect::from_center_size(card_rect.center(), vec2(48.0, 48.0));
                        theme::paint_spinner(ui, disc, 28.0, Color32::WHITE);
                    } else if current_item.media_path.is_none()
                        && current_item.raw_message.is_none()
                    {
                        let label_rect = Rect::from_center_size(
                            pos2(card_rect.center().x, card_rect.center().y + 50.0),
                            vec2(240.0, 30.0),
                        );
                        ui.painter().rect_filled(
                            label_rect,
                            CornerRadius::same(8),
                            Color32::from_black_alpha(180),
                        );
                        ui.painter().text(
                            label_rect.center(),
                            Align2::CENTER_CENTER,
                            "Video tidak tersedia (status lama)",
                            egui::FontId::proportional(12.0),
                            Color32::WHITE,
                        );
                    }
                }
            } else if let Some(ref media_path) = current_item.media_path {
                // Image story with full-resolution downloaded file
                let path = std::path::Path::new(media_path);
                if path.exists() {
                    let image = widgets::file_image(ui, path);
                    match image.load_for_size(ctx, card_rect.size()) {
                        Ok(egui::load::TexturePoll::Ready { texture }) => {
                            let fitted = fit_inside(card_rect, texture.size);
                            image
                                .fit_to_exact_size(fitted.size())
                                .corner_radius(16.0)
                                .paint_at(ui, fitted);
                        }
                        _ => {
                            if let Some(thumb_bytes) = &current_item.thumbnail {
                                render_thumbnail(ctx, ui, card_rect, &current_item.id, thumb_bytes);
                            }
                            let disc = Rect::from_center_size(card_rect.center(), vec2(48.0, 48.0));
                            theme::paint_spinner(ui, disc, 28.0, Color32::WHITE);
                        }
                    }
                } else if let Some(thumb_bytes) = &current_item.thumbnail {
                    render_thumbnail(ctx, ui, card_rect, &current_item.id, thumb_bytes);
                }
            } else if let Some(thumb_bytes) = &current_item.thumbnail {
                // Image story with thumbnail (downloading or legacy)
                render_thumbnail(ctx, ui, card_rect, &current_item.id, thumb_bytes);
                if is_image_loading {
                    let disc = Rect::from_center_size(card_rect.center(), vec2(48.0, 48.0));
                    theme::paint_spinner(ui, disc, 28.0, Color32::WHITE);
                } else if current_item.raw_message.is_none() {
                    let label_rect = Rect::from_center_size(
                        pos2(card_rect.center().x, card_rect.bottom() - 100.0),
                        vec2(220.0, 26.0),
                    );
                    ui.painter().rect_filled(
                        label_rect,
                        CornerRadius::same(6),
                        Color32::from_black_alpha(160),
                    );
                    ui.painter().text(
                        label_rect.center(),
                        Align2::CENTER_CENTER,
                        "Hanya pratinjau (status lama)",
                        egui::FontId::proportional(11.5),
                        Color32::WHITE,
                    );
                }
            } else {
                ui.painter().text(
                    card_rect.center(),
                    Align2::CENTER_CENTER,
                    "Status Update",
                    egui::FontId::proportional(20.0),
                    Color32::from_white_alpha(180),
                );
            }

            // In WhatsApp / Instagram: holding the screen hides chrome for an unobstructed view
            let hide_chrome = state.is_holding;

            if !hide_chrome {
                // 2. Top dark overlay for text contrast if media story
                if current_item.media_type.is_some() {
                    let top_overlay = Rect::from_min_size(card_rect.min, vec2(card_width, 85.0));
                    ui.painter().rect_filled(
                        top_overlay,
                        CornerRadius {
                            nw: 16,
                            ne: 16,
                            sw: 0,
                            se: 0,
                        },
                        Color32::from_black_alpha(130),
                    );
                }

                let is_my_story =
                    state.sender == "me" || state.sender == app.me.as_deref().unwrap_or("");
                // 3. Caption if present
                if let Some(caption) = &current_item.caption {
                    let cap_y = if is_my_story {
                        card_rect.bottom() - 64.0
                    } else {
                        card_rect.bottom() - 156.0
                    };
                    let cap_width = (card_width - 32.0).min(640.0);
                    let cap_rect = Rect::from_center_size(
                        pos2(card_rect.center().x, cap_y + 22.0),
                        vec2(cap_width, 44.0),
                    );
                    ui.painter().rect_filled(
                        cap_rect,
                        CornerRadius::same(12),
                        Color32::from_black_alpha(175),
                    );
                    ui.painter().text(
                        cap_rect.center(),
                        Align2::CENTER_CENTER,
                        caption,
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );
                }

                // 4. Top Progress Bars
                let seg_spacing = 4.0;
                let total_spacing = seg_spacing * (items.len().saturating_sub(1) as f32);
                let bar_width = ((card_width - 32.0) - total_spacing) / (items.len() as f32);
                let bar_y = card_rect.top() + 14.0;

                for (i, _) in items.iter().enumerate() {
                    let seg_x = card_rect.left() + 16.0 + i as f32 * (bar_width + seg_spacing);
                    let seg_rect = Rect::from_min_size(pos2(seg_x, bar_y), vec2(bar_width, 3.0));

                    // Background track
                    ui.painter().rect_filled(
                        seg_rect,
                        CornerRadius::same(2),
                        Color32::from_white_alpha(75),
                    );

                    // Filled portion
                    let fill_fraction = if i < state.index {
                        1.0
                    } else if i == state.index {
                        progress
                    } else {
                        0.0
                    };

                    if fill_fraction > 0.0 {
                        let filled_rect =
                            Rect::from_min_size(seg_rect.min, vec2(bar_width * fill_fraction, 3.0));
                        ui.painter().rect_filled(
                            filled_rect,
                            CornerRadius::same(2),
                            Color32::WHITE,
                        );
                    }
                }

                // 5. Header: Avatar, Name, Timestamp, Close Button
                let header_rect = Rect::from_min_size(
                    pos2(card_rect.left() + 16.0, card_rect.top() + 24.0),
                    vec2(card_width - 32.0, 40.0),
                );

                let avatar_center = pos2(header_rect.left() + 18.0, header_rect.center().y);
                let sender_name = current_item
                    .sender_name
                    .as_deref()
                    .unwrap_or(&current_item.sender);
                let picture = app.avatar(&current_item.sender);

                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(Rect::from_center_size(avatar_center, vec2(36.0, 36.0))),
                    |ui| {
                        widgets::avatar(
                            ui,
                            &palette,
                            sender_name,
                            &current_item.sender,
                            36.0,
                            picture.as_deref(),
                        );
                    },
                );

                // Name & time text
                let name_pos = pos2(header_rect.left() + 46.0, header_rect.top() + 4.0);
                ui.painter().text(
                    name_pos,
                    Align2::LEFT_TOP,
                    sender_name,
                    egui::FontId::proportional(14.0),
                    Color32::WHITE,
                );

                let time_pos = pos2(header_rect.left() + 46.0, header_rect.top() + 22.0);
                let time_str = format_relative_time(current_item.timestamp);
                ui.painter().text(
                    time_pos,
                    Align2::LEFT_TOP,
                    time_str,
                    egui::FontId::proportional(11.5),
                    Color32::from_white_alpha(180),
                );

                // Close button (✕)
                let close_resp = ui.allocate_rect(close_rect, Sense::click());
                if close_resp.hovered() {
                    ui.painter().circle_filled(
                        close_rect.center(),
                        14.0,
                        Color32::from_white_alpha(40),
                    );
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                ui.painter().text(
                    close_rect.center(),
                    Align2::CENTER_CENTER,
                    "✕",
                    egui::FontId::proportional(16.0),
                    Color32::WHITE,
                );
                if close_resp.clicked() {
                    app.actions.push(Action::CloseStoryViewer);
                }

                // Delete button for own story updates (Revoke)
                if is_my_story {
                    let delete_rect = close_rect.translate(vec2(-36.0, 0.0));
                    let del_resp = ui.allocate_rect(delete_rect, Sense::click());
                    if del_resp.hovered() {
                        ui.painter().circle_filled(
                            delete_rect.center(),
                            14.0,
                            Color32::from_rgb(220, 50, 50),
                        );
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    theme::paint_icon(ui, Icon::Trash, delete_rect, 15.0, Color32::WHITE);
                    if del_resp.clicked() {
                        app.actions
                            .push(Action::RevokeStory(current_item.id.clone()));
                    }
                }

                // Persistent Pause indicator in center of card (when not held)
                if state.paused && !is_media_loading {
                    let pause_center = card_rect.center();
                    ui.painter()
                        .circle_filled(pause_center, 28.0, Color32::from_black_alpha(160));
                    ui.painter().text(
                        pause_center,
                        Align2::CENTER_CENTER,
                        "⏸",
                        egui::FontId::proportional(22.0),
                        Color32::WHITE,
                    );
                }

                // 6. Quick Reactions and Reply Bar (for other users' stories)
                if !is_my_story {
                    let mut chosen_emoji: Option<&'static str> = None;
                    let mut text_to_send = None;

                    // 6A. Quick Reactions Row (floating cleanly above the reply bar)
                    let reactions_y = card_rect.bottom() - 102.0;
                    const EMOJIS: &[&str] = &["❤️", "😂", "😮", "😢", "🙏", "👏"];
                    let emoji_count = EMOJIS.len() as f32;
                    let btn_size = 36.0;
                    let gap = 12.0;
                    let total_reactions_w = emoji_count * btn_size + (emoji_count - 1.0) * gap;
                    let start_rx = card_rect.center().x - total_reactions_w / 2.0;

                    for (idx, &emoji) in EMOJIS.iter().enumerate() {
                        let rx = start_rx + idx as f32 * (btn_size + gap);
                        let r_rect =
                            Rect::from_min_size(pos2(rx, reactions_y), Vec2::splat(btn_size));
                        let r_resp = ui.allocate_rect(r_rect, Sense::click());
                        let hovered = r_resp.hovered();

                        if hovered {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }

                        let fill = if hovered {
                            Color32::from_black_alpha(225)
                        } else {
                            Color32::from_black_alpha(150)
                        };
                        let stroke_color = if hovered {
                            Color32::from_white_alpha(140)
                        } else {
                            Color32::from_white_alpha(45)
                        };
                        ui.painter()
                            .circle_filled(r_rect.center(), btn_size / 2.0, fill);
                        ui.painter().circle_stroke(
                            r_rect.center(),
                            btn_size / 2.0,
                            Stroke::new(1.0, stroke_color),
                        );

                        let emoji_scale = if hovered { 18.0 } else { 16.0 };
                        ui.painter().text(
                            r_rect.center(),
                            Align2::CENTER_CENTER,
                            emoji,
                            egui::FontId::proportional(emoji_scale),
                            Color32::WHITE,
                        );

                        if r_resp.clicked() {
                            chosen_emoji = Some(emoji);
                        }

                        r_resp.on_hover_text(format!("Balas dengan {emoji}"));
                    }

                    // 6B. Bottom Reply Input Pill Bar
                    let reply_bar_width = (card_width - 32.0).min(520.0);
                    let reply_bar_rect = Rect::from_center_size(
                        pos2(card_rect.center().x, card_rect.bottom() - 34.0),
                        vec2(reply_bar_width, 44.0),
                    );

                    // Semi-transparent pill background
                    ui.painter().rect_filled(
                        reply_bar_rect,
                        CornerRadius::same(22),
                        Color32::from_black_alpha(215),
                    );
                    ui.painter().rect_stroke(
                        reply_bar_rect,
                        CornerRadius::same(22),
                        Stroke::new(1.0, Color32::from_white_alpha(55)),
                        egui::StrokeKind::Inside,
                    );

                    if state.reply_sent {
                        // Green sent badge feedback
                        ui.painter().text(
                            reply_bar_rect.center(),
                            Align2::CENTER_CENTER,
                            "✓ Balasan terkirim",
                            theme::bold(14.0),
                            Color32::from_rgb(37, 211, 102),
                        );
                    } else {
                        // Interactive input layout
                        ui.scope_builder(egui::UiBuilder::new().max_rect(reply_bar_rect), |ui| {
                            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                                ui.add_space(14.0);

                                // Chat icon
                                let (icon_rect, _) =
                                    ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                                theme::paint_icon(
                                    ui,
                                    Icon::MessageCircle,
                                    icon_rect,
                                    16.0,
                                    Color32::from_white_alpha(150),
                                );

                                // TextEdit field without nested box border
                                let mut submit = false;
                                let available = (ui.available_width() - 48.0).max(60.0);
                                if let Some(viewer) = app.story_viewer.as_mut() {
                                    let te = egui::TextEdit::singleline(&mut viewer.reply_text)
                                        .frame(egui::Frame::NONE)
                                        .hint_text("Balas status...")
                                        .text_color(Color32::WHITE)
                                        .font(egui::FontId::proportional(14.0))
                                        .desired_width(available);
                                    let te_resp = ui.add(te);
                                    if te_resp.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    {
                                        submit = true;
                                    }
                                    if submit && !viewer.reply_text.trim().is_empty() {
                                        text_to_send = Some(viewer.reply_text.trim().to_string());
                                        viewer.reply_text.clear();
                                        viewer.reply_sent = true;
                                        viewer.reply_sent_at = Some(Instant::now());
                                    }
                                }

                                // Send circular button
                                let has_text = app
                                    .story_viewer
                                    .as_ref()
                                    .is_some_and(|v| !v.reply_text.trim().is_empty());
                                let send_btn_size = vec2(32.0, 32.0);
                                let (send_rect, send_resp) =
                                    ui.allocate_exact_size(send_btn_size, Sense::click());
                                let send_hovered = send_resp.hovered();

                                if has_text {
                                    if send_hovered {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    let fill = if send_hovered {
                                        Color32::from_rgb(0, 190, 150)
                                    } else {
                                        Color32::from_rgb(0, 168, 132)
                                    };
                                    ui.painter().circle_filled(send_rect.center(), 16.0, fill);
                                    ui.painter().text(
                                        send_rect.center(),
                                        Align2::CENTER_CENTER,
                                        "➤",
                                        egui::FontId::proportional(13.0),
                                        Color32::WHITE,
                                    );
                                } else {
                                    ui.painter().circle_filled(
                                        send_rect.center(),
                                        16.0,
                                        Color32::from_white_alpha(20),
                                    );
                                    ui.painter().text(
                                        send_rect.center(),
                                        Align2::CENTER_CENTER,
                                        "➤",
                                        egui::FontId::proportional(13.0),
                                        Color32::from_white_alpha(80),
                                    );
                                }

                                if send_resp.clicked() && has_text {
                                    if let Some(viewer) = app.story_viewer.as_mut() {
                                        if !viewer.reply_text.trim().is_empty() {
                                            text_to_send =
                                                Some(viewer.reply_text.trim().to_string());
                                            viewer.reply_text.clear();
                                            viewer.reply_sent = true;
                                            viewer.reply_sent_at = Some(Instant::now());
                                        }
                                    }
                                }
                            });
                        });
                    }

                    if let Some(emoji) = chosen_emoji {
                        app.actions.push(Action::ReplyStory {
                            sender: state.sender.clone(),
                            story_id: current_item.id.clone(),
                            text: emoji.to_string(),
                            raw_message: current_item.raw_message.clone(),
                        });
                        if let Some(viewer) = app.story_viewer.as_mut() {
                            viewer.reply_sent = true;
                            viewer.reply_sent_at = Some(Instant::now());
                        }
                    } else if let Some(txt) = text_to_send {
                        app.actions.push(Action::ReplyStory {
                            sender: state.sender.clone(),
                            story_id: current_item.id.clone(),
                            text: txt,
                            raw_message: current_item.raw_message.clone(),
                        });
                    }
                }
            }

            // Outline stroke on card
            ui.painter().rect_stroke(
                card_rect,
                CornerRadius::same(16),
                Stroke::new(1.0, Color32::from_white_alpha(40)),
                egui::StrokeKind::Inside,
            );

            // Floating buttons for desktop navigation (❮ and ❯)
            let left_btn_resp = ui.allocate_rect(left_arrow_rect, Sense::click());
            let right_btn_resp = ui.allocate_rect(right_arrow_rect, Sense::click());

            let left_bg = if left_btn_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if show_arrows_outside {
                    Color32::from_white_alpha(50)
                } else {
                    Color32::from_black_alpha(220)
                }
            } else if show_arrows_outside {
                Color32::from_white_alpha(20)
            } else {
                Color32::from_black_alpha(150)
            };
            ui.painter()
                .circle_filled(left_arrow_rect.center(), 20.0, left_bg);
            ui.painter().text(
                left_arrow_rect.center(),
                Align2::CENTER_CENTER,
                "❮",
                egui::FontId::proportional(18.0),
                Color32::WHITE,
            );
            if left_btn_resp.clicked() {
                app.actions.push(Action::PrevStory);
            }

            let right_bg = if right_btn_resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if show_arrows_outside {
                    Color32::from_white_alpha(50)
                } else {
                    Color32::from_black_alpha(220)
                }
            } else if show_arrows_outside {
                Color32::from_white_alpha(20)
            } else {
                Color32::from_black_alpha(150)
            };
            ui.painter()
                .circle_filled(right_arrow_rect.center(), 20.0, right_bg);
            ui.painter().text(
                right_arrow_rect.center(),
                Align2::CENTER_CENTER,
                "❯",
                egui::FontId::proportional(18.0),
                Color32::WHITE,
            );
            if right_btn_resp.clicked() {
                app.actions.push(Action::NextStory);
            }

            // Clicking backdrop outside card closes viewer
            if ctx.input(|i| i.pointer.primary_clicked()) {
                if let Some(pos) = pointer_pos {
                    if !card_rect.contains(pos)
                        && !left_arrow_rect.contains(pos)
                        && !right_arrow_rect.contains(pos)
                    {
                        app.actions.push(Action::CloseStoryViewer);
                    }
                }
            }
        });
}

/// Modal dialog for composing and posting a new text status to WhatsApp.
pub fn post_modal_show(app: &mut App, ctx: &egui::Context) {
    let is_open = app.post_story_open;
    let t = ctx.animate_bool_with_time(egui::Id::new("post-story-modal-anim"), is_open, 0.16);
    if !is_open && t <= 0.0 {
        return;
    }
    if !app.post_story_open {
        return;
    }

    let palette = app.palette;
    let frame = Frame::new()
        .fill(palette.overlay.gamma_multiply(t))
        .stroke(Stroke::new(1.0, palette.outline.gamma_multiply(t)))
        .corner_radius(CornerRadius::same(theme::RADIUS + 4))
        .inner_margin(Margin::same(20))
        .shadow(palette.modal_shadow());

    let response = egui::Modal::new(egui::Id::new("post-story-modal"))
        .frame(frame)
        .backdrop_color(palette.shadow.gamma_multiply(t))
        .show(ctx, |ui| {
            ui.set_opacity(t);
            ui.set_width(420.0);

            // Header row
            ui.horizontal(|ui| {
                theme::text(
                    ui,
                    crate::i18n::gettext(app.locale, "Create Status"),
                    theme::bold(17.0),
                    palette.text,
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::icon_button(
                        ui,
                        Icon::X,
                        18.0,
                        palette.secondary,
                        palette.text,
                        "Close",
                    )
                    .clicked()
                    {
                        app.actions.push(Action::ClosePostStory);
                    }
                });
            });

            ui.add_space(14.0);

            // Live preview card
            let preview_rect = ui.allocate_space(vec2(ui.available_width(), 160.0)).1;

            if let Some(ref media_path) = app.post_story_media_path {
                let ext = media_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                let is_vid = ["mp4", "mov", "mkv", "webm", "3gp"].contains(&ext.as_str());

                ui.painter().rect_filled(
                    preview_rect,
                    CornerRadius::same(12),
                    Color32::from_rgb(20, 24, 30),
                );
                ui.painter().rect_stroke(
                    preview_rect,
                    CornerRadius::same(12),
                    Stroke::new(1.0, Color32::from_white_alpha(40)),
                    egui::StrokeKind::Inside,
                );

                let file_name = media_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "media".to_string());

                if is_vid {
                    let text = format!("🎬 Video:\n{file_name}");
                    let galley = ui.painter().layout(
                        text,
                        egui::FontId::proportional(15.0),
                        Color32::WHITE,
                        preview_rect.width() - 32.0,
                    );
                    let pos = pos2(
                        preview_rect.center().x - galley.size().x / 2.0,
                        preview_rect.center().y - galley.size().y / 2.0,
                    );
                    ui.painter().galley(pos, galley, Color32::WHITE);
                } else if let Ok(bytes) = std::fs::read(media_path) {
                    if let Ok(image) = image::load_from_memory(&bytes) {
                        let rgba = image.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        let pixels = rgba.into_raw();
                        let color_img = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
                        let texture = ctx.load_texture(
                            "post-story-preview",
                            color_img,
                            egui::TextureOptions::LINEAR,
                        );
                        let img_size = vec2(size[0] as f32, size[1] as f32);
                        let fitted_rect = fit_inside(preview_rect.shrink(4.0), img_size);
                        ui.painter().image(
                            texture.id(),
                            fitted_rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }

                // Remove media button
                let remove_rect = Rect::from_min_size(
                    pos2(preview_rect.right() - 92.0, preview_rect.top() + 8.0),
                    vec2(84.0, 26.0),
                );
                let remove_resp = ui.allocate_rect(remove_rect, Sense::click());
                if remove_resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    ui.painter().rect_filled(
                        remove_rect,
                        CornerRadius::same(13),
                        palette.danger.gamma_multiply(0.85),
                    );
                } else {
                    ui.painter().rect_filled(
                        remove_rect,
                        CornerRadius::same(13),
                        Color32::from_black_alpha(170),
                    );
                }
                ui.painter().text(
                    remove_rect.center(),
                    Align2::CENTER_CENTER,
                    "✕ Remove",
                    theme::medium(12.0),
                    Color32::WHITE,
                );
                if remove_resp.clicked() {
                    app.post_story_media_path = None;
                }
            } else {
                let selected_bg = STORY_COLORS[app.post_story_color_idx % STORY_COLORS.len()].0;
                let preview_bg = argb_to_color32(selected_bg);

                ui.painter()
                    .rect_filled(preview_rect, CornerRadius::same(12), preview_bg);
                ui.painter().rect_stroke(
                    preview_rect,
                    CornerRadius::same(12),
                    Stroke::new(1.0, Color32::from_white_alpha(30)),
                    egui::StrokeKind::Inside,
                );

                let preview_text = if app.post_story_text.trim().is_empty() {
                    "Type a status…".to_string()
                } else {
                    app.post_story_text.clone()
                };

                let preview_galley = ui.painter().layout(
                    preview_text,
                    egui::FontId::proportional(20.0),
                    if app.post_story_text.trim().is_empty() {
                        Color32::from_white_alpha(140)
                    } else {
                        Color32::WHITE
                    },
                    preview_rect.width() - 32.0,
                );

                let text_pos = pos2(
                    preview_rect.center().x - preview_galley.size().x / 2.0,
                    preview_rect.center().y - preview_galley.size().y / 2.0,
                );
                ui.painter()
                    .galley(text_pos, preview_galley, Color32::WHITE);
            }

            ui.add_space(12.0);

            // Input label + file attach button
            ui.horizontal(|ui| {
                let label = if app.post_story_media_path.is_some() {
                    "Caption (optional)"
                } else {
                    "Status text"
                };
                theme::text(ui, label, theme::medium(12.0), palette.secondary);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if app.post_story_media_path.is_none()
                        && theme::soft_button(
                            ui,
                            &palette,
                            Some(Icon::Image),
                            "Attach Photo / Video",
                            false,
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        if let Some(picked) = rfd::FileDialog::new()
                            .set_title("Choose photo or video for status")
                            .add_filter(
                                "Media",
                                &["jpg", "jpeg", "png", "webp", "mp4", "mov", "mkv", "webm"],
                            )
                            .pick_file()
                        {
                            app.post_story_media_path = Some(picked);
                        }
                    }
                });
            });
            ui.add_space(4.0);

            let mut text = app.post_story_text.clone();
            let hint = if app.post_story_media_path.is_some() {
                "Add a caption…"
            } else {
                "What's on your mind?"
            };
            let text_edit = egui::TextEdit::multiline(&mut text)
                .hint_text(hint)
                .desired_rows(if app.post_story_media_path.is_some() {
                    2
                } else {
                    3
                })
                .desired_width(ui.available_width())
                .char_limit(700);

            let _edit_resp = ui.add(text_edit);
            if text != app.post_story_text {
                app.post_story_text = text;
            }

            // Only show color and font options if posting text status
            if app.post_story_media_path.is_none() {
                ui.add_space(10.0);

                // Color picker row
                theme::text(
                    ui,
                    "Background color",
                    theme::medium(12.0),
                    palette.secondary,
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                    for (idx, (color_val, name)) in STORY_COLORS.iter().enumerate() {
                        let color = argb_to_color32(*color_val);
                        let (dot_rect, dot_resp) =
                            ui.allocate_exact_size(vec2(28.0, 28.0), Sense::click());

                        if dot_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if dot_resp.clicked() {
                            app.post_story_color_idx = idx;
                        }
                        dot_resp.on_hover_text(*name);

                        ui.painter().circle_filled(dot_rect.center(), 13.0, color);

                        if app.post_story_color_idx == idx {
                            ui.painter().circle_stroke(
                                dot_rect.center(),
                                14.5,
                                Stroke::new(2.0, palette.text),
                            );
                            ui.painter().text(
                                dot_rect.center(),
                                Align2::CENTER_CENTER,
                                "✓",
                                egui::FontId::proportional(12.0),
                                Color32::WHITE,
                            );
                        }
                    }
                });

                ui.add_space(10.0);

                // Font picker row
                theme::text(ui, "Font style", theme::medium(12.0), palette.secondary);
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                    for (idx, (name, _)) in STORY_FONTS.iter().enumerate() {
                        let selected = app.post_story_font_idx == idx;
                        if theme::soft_button(ui, &palette, None, *name, selected).clicked() {
                            app.post_story_font_idx = idx;
                        }
                    }
                });
            }

            ui.add_space(16.0);

            // Bottom Buttons
            ui.horizontal(|ui| {
                let char_count = format!("{}/700", app.post_story_text.chars().count());
                theme::text(ui, char_count, theme::medium(11.5), palette.secondary);

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let can_send = app.post_story_media_path.is_some()
                        || !app.post_story_text.trim().is_empty();

                    let send_resp = ui
                        .add_enabled_ui(can_send, |ui| {
                            theme::pill_button(ui, &palette, "Send Status", true)
                        })
                        .inner;

                    if send_resp.clicked() {
                        if let Some(media_path) = app.post_story_media_path.clone() {
                            let caption = (!app.post_story_text.trim().is_empty())
                                .then(|| app.post_story_text.trim().to_string());
                            app.actions.push(Action::PostMediaStory {
                                path: media_path,
                                caption,
                            });
                        } else {
                            let text = app.post_story_text.trim().to_string();
                            let bg = STORY_COLORS[app.post_story_color_idx % STORY_COLORS.len()].0;
                            let font = STORY_FONTS[app.post_story_font_idx % STORY_FONTS.len()].1;

                            app.actions.push(Action::PostTextStory {
                                text,
                                background_argb: bg,
                                font,
                            });
                        }
                        app.actions.push(Action::ClosePostStory);
                    }

                    ui.add_space(8.0);

                    if theme::pill_button(ui, &palette, "Cancel", false).clicked() {
                        app.actions.push(Action::ClosePostStory);
                    }
                });
            });
        });

    if response.should_close() {
        app.actions.push(Action::ClosePostStory);
    }
}
