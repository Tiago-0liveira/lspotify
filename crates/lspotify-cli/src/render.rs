use std::time::Instant;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap},
};

use crate::{
    HitMap, HitTarget,
    artwork::{ArtworkManager, ArtworkPlacement},
    shortcuts::ShortcutAction,
    state::{
        AppState, EntityKind, FocusRegion, LayoutMode, LibraryTab, LoadState, MIN_HEIGHT,
        MIN_WIDTH, Overlay, Route, SearchFilter, SettingsTab,
    },
};

const INK: Color = Color::Rgb(18, 18, 25);
const INK_SOFT: Color = Color::Rgb(31, 31, 42);
const LAVENDER: Color = Color::Rgb(190, 174, 255);
const SAGE: Color = Color::Rgb(155, 207, 166);
const PEACH: Color = Color::Rgb(255, 187, 153);
const MUTED: Color = Color::Rgb(143, 143, 160);
const BORDER_SUBTLE: Color = Color::Rgb(45, 45, 60);

pub fn render(frame: &mut Frame<'_>, state: &mut AppState, hits: &mut HitMap) {
    render_inner(frame, state, hits, None);
}

pub fn render_with_artwork(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    hits: &mut HitMap,
    artwork: &mut ArtworkManager,
) {
    render_inner(frame, state, hits, Some(artwork));
}

#[allow(clippy::too_many_lines)]
fn render_inner(
    frame: &mut Frame<'_>,
    state: &mut AppState,
    hits: &mut HitMap,
    mut artwork: Option<&mut ArtworkManager>,
) {
    hits.clear();
    let area = frame.area();
    state.update_layout(area.width, area.height);
    let artwork_visible = state.overlay.is_none() || state.artwork_under_overlays;
    if !artwork_visible && let Some(manager) = artwork.as_deref_mut() {
        manager.clear_placement();
    }
    frame.render_widget(Block::default().style(Style::default().bg(INK).fg(Color::White)), area);
    match state.layout {
        LayoutMode::Resize => {
            render_resize(frame, area, state);
        }
        LayoutMode::SidePlayer => {
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(10)])
                .split(area);
            render_topbar(frame, vertical[0], state, hits);
            let body = vertical[1];
            let player_width = (body.width / 3).clamp(36, 44);
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Fill(1), Constraint::Length(player_width)])
                .split(body);
            render_content(
                frame,
                columns[0],
                state,
                hits,
                if artwork_visible { artwork.as_deref_mut() } else { None },
            );
            render_player(
                frame,
                columns[1],
                state,
                hits,
                if artwork_visible { artwork } else { None },
            );
            render_overlay(frame, state, hits);
        }
        LayoutMode::StackedQueue => {
            let queue_height =
                if state.queue_visible() { (area.height / 4).clamp(8, 12) } else { 0 };
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),
                    Constraint::Min(10),
                    Constraint::Length(queue_height),
                    Constraint::Length(11),
                ])
                .split(area);
            render_topbar(frame, vertical[0], state, hits);
            render_content(
                frame,
                vertical[1],
                state,
                hits,
                if artwork_visible { artwork.as_deref_mut() } else { None },
            );
            if state.queue_visible() {
                render_queue(frame, vertical[2], state, hits);
            }
            render_player(
                frame,
                vertical[3],
                state,
                hits,
                if artwork_visible { artwork } else { None },
            );
            render_overlay(frame, state, hits);
        }
        LayoutMode::Compact | LayoutMode::Standard | LayoutMode::Wide => {
            let vertical = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(2), Constraint::Min(10), Constraint::Length(11)])
                .split(area);
            let body = vertical[1];
            render_topbar(frame, vertical[0], state, hits);
            match state.layout {
                LayoutMode::Compact => {
                    let header = Rect { x: body.x, y: body.y, width: body.width, height: 0 };
                    if header.height > 0 {
                        hits.add(
                            Rect { x: header.x, y: header.y, width: 8, height: header.height },
                            HitTarget::MenuButton,
                        );
                    }
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled(
                                " ☰ Menu ",
                                Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(state.page.route.label(), Style::default().fg(PEACH)),
                        ]))
                        .block(
                            Block::default()
                                .borders(Borders::BOTTOM)
                                .border_style(Style::default().fg(BORDER_SUBTLE)),
                        ),
                        header,
                    );
                    let content = body;
                    render_content(
                        frame,
                        content,
                        state,
                        hits,
                        if artwork_visible { artwork.as_deref_mut() } else { None },
                    );
                }
                LayoutMode::Standard => {
                    let columns = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Length(0), Constraint::Min(40)])
                        .split(body);
                    render_sidebar(frame, columns[0], state, hits);
                    render_content(
                        frame,
                        columns[1],
                        state,
                        hits,
                        if artwork_visible { artwork.as_deref_mut() } else { None },
                    );
                }
                LayoutMode::Wide => {
                    // Let a wide queue grow with the terminal, while always leaving a useful main pane.
                    let queue_width = if state.queue_visible() {
                        (body.width / 3).clamp(34, body.width.saturating_sub(60))
                    } else {
                        0
                    };
                    let columns = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([
                            Constraint::Length(0),
                            Constraint::Min(50),
                            Constraint::Length(queue_width),
                        ])
                        .split(body);
                    render_sidebar(frame, columns[0], state, hits);
                    render_content(
                        frame,
                        columns[1],
                        state,
                        hits,
                        if artwork_visible { artwork.as_deref_mut() } else { None },
                    );
                    if state.queue_visible() {
                        render_queue(frame, columns[2], state, hits);
                    }
                }
                _ => unreachable!(),
            }
            render_player(
                frame,
                vertical[2],
                state,
                hits,
                if artwork_visible { artwork } else { None },
            );
            render_overlay(frame, state, hits);
        }
    }
}

fn shortcut_label(state: &AppState, action: ShortcutAction) -> String {
    state.shortcuts.primary_label(action)
}

fn shortcut_list_label(state: &AppState, action: ShortcutAction, separator: &str) -> String {
    state.shortcuts.list_label(action, separator)
}

fn render_topbar(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    let home_key = shortcut_label(state, ShortcutAction::Home);
    let search_key = shortcut_label(state, ShortcutAction::Search);
    let library_key = shortcut_label(state, ShortcutAction::Library);
    let settings_key = shortcut_label(state, ShortcutAction::Settings);
    let mut routes = vec![
        (home_key, "⌂ Home", Route::Home),
        (search_key, "⌕ Search", Route::Search),
        (library_key, "▣ Library", Route::Library),
        (settings_key, "⚙ Settings", Route::Settings),
    ];
    if !state.layout_has_inline_queue() {
        let queue_key = shortcut_label(state, ShortcutAction::Queue);
        routes.push((queue_key, "☷ Queue", Route::Queue));
    }
    let (title_prefix, prefix_cells) =
        if area.width < 85 { (" ♪ ", 3) } else { (" ♪ lspotify  ", 13) };
    let mut spans = vec![Span::styled(
        title_prefix,
        Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD),
    )];
    let mut x = area.x.saturating_add(prefix_cells);
    for (key, label, route) in routes {
        let active = route == state.primary_section;
        let item_text = format!(" [{key}] {label} ");
        let width = u16::try_from(item_text.chars().count()).unwrap_or(u16::MAX);
        if active {
            spans.push(Span::styled(
                item_text,
                Style::default().bg(LAVENDER).fg(INK).add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(" [", Style::default().fg(MUTED)));
            spans.push(Span::styled(key, Style::default().fg(PEACH)));
            spans.push(Span::styled(format!("] {label} "), Style::default().fg(MUTED)));
        }
        hits.add(Rect { x, y: area.y, width, height: area.height }, HitTarget::TopNav(route));
        x = x.saturating_add(width);
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER_SUBTLE))
                .style(Style::default().bg(INK)),
        ),
        area,
    );
}

fn render_resize(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let quit_hint = state.shortcuts.format_quit_hint();
    let text = format!(
        "lspotify needs at least {MIN_WIDTH}×{MIN_HEIGHT}\nCurrent: {}×{}\n\nPress {quit_hint} to quit",
        area.width, area.height
    );
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(PEACH))
            .wrap(Wrap { trim: true }),
        centered(60, 8, area),
    );
}

fn focus_block(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(if focused { LAVENDER } else { BORDER_SUBTLE }))
        .style(Style::default().bg(INK_SOFT))
}

fn render_sidebar(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    hits.sidebar = Some(area);
    let items = AppState::NAVIGATION
        .iter()
        .enumerate()
        .map(|(index, route)| {
            let active = same_root(route, &state.page.route);
            let selected = state.sidebar.selected == index;
            let marker = if active {
                "●"
            } else if selected {
                "›"
            } else {
                " "
            };
            let style = if active {
                Style::default().fg(PEACH).add_modifier(Modifier::BOLD)
            } else if selected && state.focus == FocusRegion::Sidebar {
                Style::default().fg(LAVENDER)
            } else {
                Style::default().fg(Color::White)
            };
            hits.add(
                Rect {
                    x: area.x + 1,
                    y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
                HitTarget::Sidebar(index),
            );
            ListItem::new(Line::from(Span::styled(format!(" {marker} {}", route.label()), style)))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(focus_block(" Navigation ", state.focus == FocusRegion::Sidebar)),
        area,
    );
}

fn same_root(left: &Route, right: &Route) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

#[allow(clippy::too_many_lines)]
fn render_content(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut AppState,
    hits: &mut HitMap,
    artwork: Option<&mut ArtworkManager>,
) {
    hits.content = Some(area);
    let has_detail = matches!(
        state.page.route,
        Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. }
    );
    let header_height = if has_detail {
        if area.height < 16 { 5 } else { 10 }
    } else if matches!(state.page.route, Route::Search | Route::Library | Route::Settings) {
        4
    } else {
        3
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(2)])
        .split(area);
    render_content_header(frame, chunks[0], state, hits);
    let selected_art = state.page.selected_item().and_then(|item| item.artwork_url.as_deref());
    let should_show_art = has_detail || selected_art.is_some();
    if should_show_art && let Some(artwork) = artwork {
        let (placement, url, placeholder) = if has_detail {
            (
                ArtworkPlacement::Header,
                state.page.artwork_url.as_deref().or(selected_art),
                match state.page.route {
                    Route::Artist { .. } => "ARTIST",
                    Route::Playlist { .. } => "PLAYLIST",
                    _ => "ALBUM",
                },
            )
        } else {
            (ArtworkPlacement::Preview, selected_art, "ART")
        };
        let max_art_w = chunks[0].width.saturating_sub(2);
        let art_area = Rect {
            x: chunks[0].x + 1,
            y: chunks[0].y + 1,
            width: if has_detail { if area.height < 16 { 6 } else { 15 } } else { 6 }
                .min(max_art_w),
            height: chunks[0].height.saturating_sub(2),
        };
        artwork.render(frame, art_area, placement, url, placeholder);
    }
    let list_area = chunks[1];
    state.content_height = usize::from(list_area.height.saturating_sub(2));
    let filtered = state.page.flattened();
    let rows = filtered
        .iter()
        .map(|item| {
            let section = state
                .page
                .sections
                .iter()
                .find(|section| {
                    section.items.iter().any(|candidate| std::ptr::eq(candidate, *item))
                })
                .map_or("", |section| section.title.as_str());
            let is_liked_song_tab = state.page.route == Route::Library
                && state.page.library_tab == LibraryTab::LikedSongs;
            let marker = match item.kind {
                EntityKind::Track => {
                    if is_liked_song_tab {
                        "♥"
                    } else {
                        match item.saved {
                            Some(true) => "♥",
                            Some(false) => " ",
                            None => "♪",
                        }
                    }
                }
                EntityKind::Album => "▣",
                EntityKind::Artist => "●",
                EntityKind::Playlist => "≡",
                EntityKind::Device => "◉",
                EntityKind::Action => "›",
                EntityKind::Message => "·",
            };
            let unavailable = if item.available { "" } else { " unavailable" };
            let duration =
                item.duration_ms.map_or_else(String::new, |ms| format!("  {}", clock(ms)));
            let is_liked = marker == "♥";
            if state.page.route == Route::Settings && state.settings_tab == SettingsTab::Shortcuts {
                ListItem::new(Line::from(vec![
                    Span::styled(" › ", Style::default().fg(PEACH)),
                    Span::styled(
                        format!("{:<26}", item.title),
                        Style::default().fg(if item.available { Color::White } else { MUTED }),
                    ),
                    Span::styled(
                        item.subtitle.clone(),
                        Style::default().fg(if item.subtitle == "(unbound)" {
                            MUTED
                        } else {
                            PEACH
                        }),
                    ),
                ]))
            } else {
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {marker} "),
                        Style::default().fg(if is_liked {
                            Color::Rgb(255, 120, 150)
                        } else if item.available {
                            PEACH
                        } else {
                            MUTED
                        }),
                    ),
                    Span::styled(
                        if section.is_empty() { String::new() } else { format!("{section} · ") },
                        Style::default().fg(MUTED),
                    ),
                    Span::styled(
                        item.title.clone(),
                        Style::default().fg(if item.available { Color::White } else { MUTED }),
                    ),
                    Span::styled(
                        format!(" — {}{duration}{unavailable}", item.subtitle),
                        Style::default().fg(MUTED),
                    ),
                ]))
            }
        })
        .collect::<Vec<_>>();
    let visible_rows =
        rows.len().saturating_sub(state.page.cursor.offset).min(state.content_height);
    for visible in 0..visible_rows {
        let absolute = state.page.cursor.offset + visible;
        hits.add(
            Rect {
                x: list_area.x + 1,
                y: list_area.y + 1 + u16::try_from(visible).unwrap_or(u16::MAX),
                width: list_area.width.saturating_sub(2),
                height: 1,
            },
            HitTarget::ContentRow(absolute),
        );
    }
    let detail_label = match state.page.route {
        Route::Artist { .. } => "Songs",
        _ => "Tracks",
    };
    let title = match &state.page.state {
        LoadState::Loading => {
            if has_detail {
                format!(" {detail_label} · loading… ")
            } else {
                format!(" {} · loading… ", state.page.title)
            }
        }
        LoadState::Partial(message) | LoadState::Stale(message) => {
            if has_detail {
                format!(" {detail_label} · {message} ")
            } else {
                format!(" {} · {message} ", state.page.title)
            }
        }
        _ => {
            if has_detail {
                format!(" {detail_label} ")
            } else {
                format!(" {} ", state.page.title)
            }
        }
    };
    let mut list_state = ListState::default()
        .with_selected((!rows.is_empty()).then_some(state.page.cursor.selected));
    *list_state.offset_mut() = state.page.cursor.offset;
    frame.render_stateful_widget(
        List::new(rows)
            .highlight_symbol("▸ ")
            .highlight_style(
                Style::default()
                    .bg(Color::Rgb(63, 55, 84))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .block(focus_block(title, state.focus == FocusRegion::Content)),
        list_area,
        &mut list_state,
    );
    if filtered.is_empty() {
        render_empty_state(frame, list_area, &state.page.state);
    }
}

#[allow(clippy::too_many_lines)]
fn render_content_header(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    let selected_art = state.page.selected_item().and_then(|item| item.artwork_url.as_deref());
    let has_detail = matches!(
        state.page.route,
        Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. }
    );
    let padding = if has_detail {
        if area.height < 8 { 8 } else { 16 }
    } else if selected_art.is_some() {
        7
    } else {
        1
    };
    if has_detail {
        render_detail_content_header(frame, area, padding, state, hits);
        return;
    }
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!(" {} ", state.page.route.label()),
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ),
        Span::styled(state.page.subtitle.clone(), Style::default().fg(MUTED)),
    ])];
    match state.page.route {
        Route::Search => {
            let field = format!(
                " Search: {}{}",
                state.search_query,
                if state.text_entry { "▏" } else { "" }
            );
            lines.push(Line::from(Span::styled(
                field,
                Style::default().fg(if state.text_entry { LAVENDER } else { Color::White }),
            )));
            lines.push(Line::from(
                SearchFilter::ALL
                    .into_iter()
                    .map(|filter| {
                        Span::styled(
                            format!(" {} ", filter.label()),
                            if filter == state.page.search_filter {
                                Style::default().bg(LAVENDER).fg(INK)
                            } else {
                                Style::default().fg(MUTED)
                            },
                        )
                    })
                    .collect::<Vec<_>>(),
            ));
            hits.add(
                Rect { x: area.x, y: area.y + 1, width: area.width, height: 1 },
                HitTarget::SearchField,
            );
        }
        Route::Library => {
            lines.push(Line::from(vec![
                Span::styled(
                    " Liked Songs ",
                    if state.page.library_tab == LibraryTab::LikedSongs {
                        Style::default().bg(LAVENDER).fg(INK)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                Span::styled(
                    " Albums ",
                    if state.page.library_tab == LibraryTab::Albums {
                        Style::default().bg(LAVENDER).fg(INK)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                Span::styled(
                    " Playlists ",
                    if state.page.library_tab == LibraryTab::Playlists {
                        Style::default().bg(LAVENDER).fg(INK)
                    } else {
                        Style::default().fg(MUTED)
                    },
                ),
                Span::styled(
                    format!("  Filter: {}", state.page.filter),
                    Style::default().fg(MUTED),
                ),
            ]));
        }
        Route::Settings => {
            let tab_style = |tab| {
                if state.settings_tab == tab {
                    Style::default().bg(LAVENDER).fg(INK)
                } else {
                    Style::default().fg(MUTED)
                }
            };
            lines.push(Line::from(vec![
                Span::styled(" General ", tab_style(SettingsTab::General)),
                Span::raw(" "),
                Span::styled(" Playback ", tab_style(SettingsTab::Playback)),
                Span::raw(" "),
                Span::styled(" Shortcuts ", tab_style(SettingsTab::Shortcuts)),
            ]));
            hits.add(
                Rect { x: area.x + padding, y: area.y + 1, width: 9, height: 1 },
                HitTarget::SettingsTab(SettingsTab::General),
            );
            hits.add(
                Rect { x: area.x + padding + 10, y: area.y + 1, width: 10, height: 1 },
                HitTarget::SettingsTab(SettingsTab::Playback),
            );
            hits.add(
                Rect { x: area.x + padding + 21, y: area.y + 1, width: 11, height: 1 },
                HitTarget::SettingsTab(SettingsTab::Shortcuts),
            );
        }
        _ => {}
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default().padding(Padding::left(padding)).style(Style::default().bg(INK)),
        ),
        area,
    );
}

fn render_detail_content_header(
    frame: &mut Frame<'_>,
    area: Rect,
    padding: u16,
    state: &AppState,
    hits: &mut HitMap,
) {
    frame.render_widget(Block::default().style(Style::default().bg(INK)), area);

    let content_x = area.x + padding;
    let content_w = area.width.saturating_sub(padding);
    if content_w == 0 {
        return;
    }

    // Row 0: Entity tag badge (Peach bold, non-duplicated)
    let kind_label = match state.page.route {
        Route::Artist { .. } => "Artist",
        Route::Playlist { .. } => "Playlist",
        _ => "Album",
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {kind_label} "),
            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
        ))),
        Rect { x: content_x, y: area.y, width: content_w, height: 1 },
    );

    // Row 1: Type icon and title
    let icon = match state.page.route {
        Route::Artist { .. } => "  ◉  ",
        Route::Playlist { .. } => "  ≡  ",
        _ => "  ▣  ",
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(icon, Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)),
            Span::styled(
                &state.page.title,
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            ),
        ])),
        Rect { x: content_x, y: area.y + 1, width: content_w, height: 1 },
    );

    // Row 2: Subtitle metadata (single non-duplicated line)
    if !state.page.subtitle.is_empty() && area.height >= 3 {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                &state.page.subtitle,
                Style::default().fg(MUTED),
            ))),
            Rect { x: content_x, y: area.y + 2, width: content_w, height: 1 },
        );
    }

    // Row 4: Action buttons
    if area.height >= 5 {
        render_detail_buttons(frame, area, content_x, content_w, state, hits);
    }

    // Row 6: Keybind hints moved to the bottom of the header section
    if area.height >= 7 {
        let enter = shortcut_label(state, ShortcutAction::Activate);
        let detail_play = shortcut_label(state, ShortcutAction::DetailPlay);
        let detail_shuffle = shortcut_label(state, ShortcutAction::DetailShuffle);
        let detail_spotify = shortcut_label(state, ShortcutAction::DetailOpenSpotify);
        let add_queue = shortcut_label(state, ShortcutAction::AddToQueue);
        let inspect = shortcut_label(state, ShortcutAction::Inspect);
        let hint = format!(
            " {enter} plays · {detail_play} play · {detail_shuffle} shuffle · {detail_spotify} spotify · {add_queue} queues · {inspect} inspects · right-click actions "
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(SAGE)))),
            Rect { x: content_x, y: area.y + 6, width: content_w, height: 1 },
        );
    }
}

fn render_detail_buttons(
    frame: &mut Frame<'_>,
    area: Rect,
    content_x: u16,
    content_w: u16,
    state: &AppState,
    hits: &mut HitMap,
) {
    let (play_label, shuffle_label, spotify_label) = if content_w >= 67 {
        (
            match state.page.route {
                Route::Artist { .. } => "[ ▶ Play artist ]",
                Route::Playlist { .. } => "[ ▶ Play playlist ]",
                _ => "[ ▶ Play album ]",
            },
            "[ ⇄ Play with shuffle ]",
            "[ ↗ Open in Spotify ]",
        )
    } else {
        ("[ ▶ Play ]", "[ ⇄ Shuffle ]", "[ ↗ Spotify ]")
    };

    let play_w = u16::try_from(play_label.chars().count()).unwrap_or(10);
    let shuffle_w = u16::try_from(shuffle_label.chars().count()).unwrap_or(13);
    let spotify_w = u16::try_from(spotify_label.chars().count()).unwrap_or(13);

    let btn_y = area.y + 4;
    let mut cur_x = content_x;

    // 1. Play button
    if cur_x + play_w <= area.x + area.width {
        let play_rect = Rect { x: cur_x, y: btn_y, width: play_w, height: 1 };
        frame.render_widget(
            Paragraph::new(play_label).style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(63, 55, 84))
                    .add_modifier(Modifier::BOLD),
            ),
            play_rect,
        );
        hits.add(play_rect, HitTarget::DetailPlay);
        cur_x += play_w + 2;
    }

    // 2. Play with shuffle button
    if cur_x + shuffle_w <= area.x + area.width {
        let shuffle_rect = Rect { x: cur_x, y: btn_y, width: shuffle_w, height: 1 };
        frame.render_widget(
            Paragraph::new(shuffle_label).style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(45, 42, 60))
                    .add_modifier(Modifier::BOLD),
            ),
            shuffle_rect,
        );
        hits.add(shuffle_rect, HitTarget::DetailShuffle);
        cur_x += shuffle_w + 2;
    }

    // 3. Open in Spotify button (rendered whenever external_url is present or derivable)
    let has_external = state.page.external_url.is_some()
        || matches!(
            state.page.route,
            Route::Album { .. } | Route::Playlist { .. } | Route::Artist { .. }
        );
    if has_external && cur_x + spotify_w <= area.x + area.width {
        let spotify_rect = Rect { x: cur_x, y: btn_y, width: spotify_w, height: 1 };
        frame.render_widget(
            Paragraph::new(spotify_label).style(
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(45, 42, 60))
                    .add_modifier(Modifier::BOLD),
            ),
            spotify_rect,
        );
        hits.add(spotify_rect, HitTarget::DetailOpenSpotify);
    }
}

fn render_empty_state(frame: &mut Frame<'_>, area: Rect, state: &LoadState) {
    let text = match state {
        LoadState::Loading => "Loading…",
        LoadState::Empty(message)
        | LoadState::Failed(message)
        | LoadState::Restricted(message)
        | LoadState::Stale(message)
        | LoadState::Partial(message) => message,
        LoadState::Ready => "Nothing here yet",
    };
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .style(Style::default().fg(match state {
                LoadState::Failed(_) | LoadState::Restricted(_) => PEACH,
                _ => MUTED,
            }))
            .wrap(Wrap { trim: true }),
        area.inner(ratatui::layout::Margin { horizontal: 2, vertical: 2 }),
    );
}

fn render_queue(frame: &mut Frame<'_>, area: Rect, state: &mut AppState, hits: &mut HitMap) {
    hits.queue = Some(area);
    let visible = usize::from(area.height.saturating_sub(5)).max(1);
    state.queue_height = visible;
    let max_offset = state.queue_upcoming.len().saturating_sub(visible);
    state.queue.offset = state.queue.offset.min(max_offset);
    let mut rows = Vec::new();
    rows.push(ListItem::new(Line::from(Span::styled(
        " Now playing",
        Style::default().fg(SAGE).add_modifier(Modifier::BOLD),
    ))));
    let now_playing =
        state.queue_now.as_ref().map(|now| (now.title.as_str(), now.subtitle.as_str())).or_else(
            || {
                if state.playback.track_uri.is_some() && !state.playback.title.is_empty() {
                    Some((state.playback.title.as_str(), state.playback.artist.as_str()))
                } else {
                    None
                }
            },
        );
    if let Some((title, subtitle)) = now_playing {
        rows.push(ListItem::new(format!(" ♪ {title} — {subtitle}")));
    } else {
        rows.push(ListItem::new(Span::styled(" Nothing playing", Style::default().fg(MUTED))));
    }
    rows.push(ListItem::new(Line::from(Span::styled(
        " Up next",
        Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
    ))));
    for (visible_index, (index, item)) in
        state.queue_upcoming.iter().enumerate().skip(state.queue.offset).take(visible).enumerate()
    {
        let selected = index == state.queue.selected;
        rows.push(ListItem::new(Span::styled(
            format!(" {}. {} — {}", index + 1, item.title, item.subtitle),
            if selected {
                Style::default().bg(Color::Rgb(63, 55, 84)).fg(Color::White)
            } else {
                Style::default()
            },
        )));
        hits.add(
            Rect {
                x: area.x + 1,
                y: area.y + 4 + u16::try_from(visible_index).unwrap_or(u16::MAX),
                width: area.width.saturating_sub(2),
                height: 1,
            },
            HitTarget::QueueRow(index),
        );
    }
    frame.render_widget(
        List::new(rows).block(focus_block(" Queue ", state.focus == FocusRegion::Queue)),
        area,
    );
}

fn render_player_status(
    frame: &mut Frame<'_>,
    status_row: Rect,
    state: &AppState,
    hits: &mut HitMap,
) {
    if status_row.width == 0 || status_row.height == 0 {
        return;
    }
    let help = if status_row.width < 45 { "?" } else { "? Help" };
    let device_name = state.display_device_name();
    let prefix = "Device: ";
    let separator = "  ·  ";
    let device_max = status_row.width.saturating_sub(
        cell_width(prefix).saturating_add(cell_width(separator)).saturating_add(cell_width(help)),
    );
    let shown_device = truncate_cells(device_name, device_max);
    let device_text = format!("{prefix}{shown_device}");
    let group_width = cell_width(&device_text)
        .saturating_add(cell_width(separator))
        .saturating_add(cell_width(help))
        .min(status_row.width);
    let group_x = status_row.x.saturating_add(status_row.width.saturating_sub(group_width));
    let device_rect =
        Rect { x: group_x, width: cell_width(&device_text).min(status_row.width), ..status_row };
    let help_rect = Rect {
        x: status_row.x.saturating_add(status_row.width.saturating_sub(cell_width(help))),
        width: cell_width(help).min(status_row.width),
        ..status_row
    };
    let notice_width = group_x.saturating_sub(status_row.x).saturating_sub(1);
    let default_notice = if (5..8).contains(&notice_width) { "Ready" } else { "lspotify" };
    let notice = state
        .backend_error
        .as_ref()
        .or(state.notice.as_ref())
        .map_or(default_notice, |notice| notice.text.as_str());
    let notice_style = state.backend_error.as_ref().or(state.notice.as_ref()).map_or(
        Style::default().fg(MUTED),
        |notice| {
            Style::default().fg(match notice.kind {
                crate::state::NoticeKind::Success => SAGE,
                crate::state::NoticeKind::Pending => LAVENDER,
                crate::state::NoticeKind::Error => PEACH,
                crate::state::NoticeKind::Info => MUTED,
            })
        },
    );
    frame.render_widget(
        Paragraph::new(truncate_cells(notice, notice_width)).style(notice_style),
        Rect { width: notice_width, ..status_row },
    );
    frame.render_widget(Paragraph::new(device_text).style(Style::default().fg(MUTED)), device_rect);
    let sep_x = device_rect.x.saturating_add(device_rect.width);
    let sep_w = cell_width(separator).min(help_rect.x.saturating_sub(sep_x));
    if sep_w > 0 {
        frame.render_widget(
            Paragraph::new(separator).style(Style::default().fg(BORDER_SUBTLE)),
            Rect { x: sep_x, width: sep_w, ..status_row },
        );
    }
    frame.render_widget(Paragraph::new(help).style(Style::default().fg(MUTED)), help_rect);
    if device_rect.width > 0 {
        hits.add(device_rect, HitTarget::PlayerDevice);
    }
    if help_rect.width > 0 {
        hits.add(help_rect, HitTarget::Help);
    }
}

#[allow(clippy::too_many_lines)]
fn render_player_vertical(
    frame: &mut Frame<'_>,
    inner: Rect,
    state: &AppState,
    hits: &mut HitMap,
    artwork: Option<&mut ArtworkManager>,
) {
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    let progress = state.playback.progress_at(Instant::now());

    let status_row =
        Rect { y: inner.y.saturating_add(inner.height.saturating_sub(1)), height: 1, ..inner };
    render_player_status(frame, status_row, state, hits);

    if inner.height <= 2 {
        return;
    }

    let deck =
        Rect { x: inner.x, y: inner.y, width: inner.width, height: inner.height.saturating_sub(1) };

    let show_album = deck.height >= 14 && !state.playback.album.is_empty();
    let num_controls_rows = if show_album { 6 } else { 5 };
    let max_art_h = deck
        .height
        .saturating_sub(num_controls_rows)
        .clamp(4, 10)
        .min(deck.height.saturating_sub(num_controls_rows));
    let max_art_w = inner.width.saturating_sub(2);

    let (art_w, art_h) = if let Some(ref mgr) = artwork {
        mgr.square_cell_size(max_art_w, max_art_h)
    } else {
        crate::artwork::square_cell_size_with_font(max_art_w, max_art_h, 8, 16)
    };

    let art_x = inner.x.saturating_add((inner.width.saturating_sub(art_w)) / 2);
    let art_area = Rect { x: art_x, y: deck.y, width: art_w, height: art_h };

    if let Some(artwork) = artwork {
        artwork.render(
            frame,
            art_area,
            ArtworkPlacement::Player,
            state.playback.artwork_url.as_deref(),
            "♪",
        );
    } else {
        frame.render_widget(
            Paragraph::new("♪").alignment(Alignment::Center).style(Style::default().fg(MUTED)),
            art_area,
        );
    }

    let remaining = Rect {
        x: inner.x,
        y: deck.y.saturating_add(art_h),
        width: inner.width,
        height: deck.height.saturating_sub(art_h),
    };

    if remaining.height == 0 {
        return;
    }

    let constraints = if show_album {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    };

    let chunks =
        Layout::default().direction(Direction::Vertical).constraints(constraints).split(remaining);

    let (title_area, artist_area, album_area, buttons_area, progress_area, volume_area) =
        if show_album {
            (chunks[0], chunks[1], Some(chunks[2]), chunks[4], chunks[6], chunks[7])
        } else {
            (chunks[0], chunks[1], None, chunks[3], chunks[5], chunks[6])
        };

    let title =
        if state.playback.title.is_empty() { "Nothing playing" } else { &state.playback.title };
    frame.render_widget(
        Paragraph::new(truncate_cells(title, inner.width))
            .alignment(Alignment::Center)
            .style(Style::default().add_modifier(Modifier::BOLD)),
        title_area,
    );

    let artist_str = &state.playback.artist;
    let artist_w = cell_width(artist_str).min(inner.width);
    frame.render_widget(
        Paragraph::new(truncate_cells(artist_str, inner.width))
            .alignment(Alignment::Center)
            .style(Style::default().fg(LAVENDER)),
        artist_area,
    );
    let start_x = artist_area.x.saturating_add((artist_area.width.saturating_sub(artist_w)) / 2);
    let mut ax = start_x;
    for (name, uri) in &state.playback.artists {
        let w = cell_width(name)
            .min(artist_area.x.saturating_add(artist_area.width).saturating_sub(ax));
        if w > 0
            && let Some(uri) = uri
        {
            hits.add(
                Rect { x: ax, width: w, ..artist_area },
                HitTarget::PlayerArtist { title: name.clone(), uri: uri.clone() },
            );
        }
        ax = ax.saturating_add(cell_width(name)).saturating_add(2);
    }

    if let Some(album_area) = album_area {
        let album_str = &state.playback.album;
        let album_w = cell_width(album_str).min(inner.width);
        frame.render_widget(
            Paragraph::new(truncate_cells(album_str, inner.width))
                .alignment(Alignment::Center)
                .style(Style::default().fg(PEACH)),
            album_area,
        );
        if let Some(uri) = &state.playback.album_uri {
            let ax = album_area.x.saturating_add((album_area.width.saturating_sub(album_w)) / 2);
            if album_w > 0 {
                hits.add(
                    Rect { x: ax, width: album_w, ..album_area },
                    HitTarget::PlayerAlbum { title: album_str.clone(), uri: uri.clone() },
                );
            }
        }
    }

    render_player_buttons(frame, buttons_area, state, hits);
    render_progress_bar(frame, progress_area, state, progress, hits, true);

    let vol_width = 18.min(volume_area.width);
    let vol_x = volume_area.x.saturating_add((volume_area.width.saturating_sub(vol_width)) / 2);
    let vol_rect = Rect { x: vol_x, width: vol_width, ..volume_area };
    render_volume_widget(frame, vol_rect, state.playback.volume, hits, true);
}

#[allow(clippy::too_many_lines)]
fn render_player(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    hits: &mut HitMap,
    artwork: Option<&mut ArtworkManager>,
) {
    frame.render_widget(focus_block(" Player ", state.focus == FocusRegion::Player), area);
    let inner = Block::default().borders(Borders::ALL).inner(area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if state.layout == LayoutMode::SidePlayer || (area.width < 55 && area.height >= 14) {
        render_player_vertical(frame, inner, state, hits, artwork);
        return;
    }
    let progress = state.playback.progress_at(Instant::now());
    let art_height = inner.height.saturating_sub(1);
    let art_area = Rect { width: inner.width.min(12), height: art_height, ..inner };
    let controls = Rect {
        x: inner.x.saturating_add(13).min(inner.x.saturating_add(inner.width)),
        y: inner.y,
        width: inner.width.saturating_sub(13),
        height: inner.height,
    };
    if controls.width > 0 {
        let available_height = controls.height.saturating_sub(1);
        let row = |offset| Rect { y: controls.y.saturating_add(offset), height: 1, ..controls };

        if available_height == 1 {
            render_player_compact_row(frame, row(0), state, progress, hits);
        } else if available_height > 1 {
            let (title_row, artist_row, album_row, buttons_row, progress_row) =
                match available_height {
                    2 => (None, None, None, Some(0), Some(1)),
                    3 => (Some(0), None, None, Some(1), Some(2)),
                    4 => (Some(0), Some(1), None, Some(2), Some(3)),
                    5 => (Some(0), Some(1), Some(2), Some(3), Some(4)),
                    6 => (Some(0), Some(1), Some(2), Some(4), Some(5)),
                    _ => (Some(0), Some(1), Some(2), Some(4), Some(6)),
                };

            if let Some(r) = title_row {
                let title = if state.playback.title.is_empty() {
                    "Nothing playing"
                } else {
                    &state.playback.title
                };
                frame.render_widget(
                    Paragraph::new(truncate_cells(title, controls.width))
                        .style(Style::default().add_modifier(Modifier::BOLD)),
                    row(r),
                );
            }

            if let Some(r) = artist_row {
                let artist_area = row(r);
                frame.render_widget(
                    Paragraph::new(truncate_cells(&state.playback.artist, controls.width))
                        .style(Style::default().fg(LAVENDER)),
                    artist_area,
                );
                let mut artist_x = artist_area.x;
                for (name, uri) in &state.playback.artists {
                    let width = cell_width(name).min(
                        artist_area.x.saturating_add(artist_area.width).saturating_sub(artist_x),
                    );
                    if width > 0
                        && let Some(uri) = uri
                    {
                        hits.add(
                            Rect { x: artist_x, width, ..artist_area },
                            HitTarget::PlayerArtist { title: name.clone(), uri: uri.clone() },
                        );
                    }
                    artist_x = artist_x.saturating_add(cell_width(name).saturating_add(2));
                }
            }

            if let Some(r) = album_row {
                let album_area = row(r);
                frame.render_widget(
                    Paragraph::new(truncate_cells(&state.playback.album, controls.width))
                        .style(Style::default().fg(PEACH)),
                    album_area,
                );
                if let Some(uri) = &state.playback.album_uri {
                    let width = cell_width(&state.playback.album).min(album_area.width);
                    if width > 0 {
                        hits.add(
                            Rect { width, ..album_area },
                            HitTarget::PlayerAlbum {
                                title: state.playback.album.clone(),
                                uri: uri.clone(),
                            },
                        );
                    }
                }
            }

            if let Some(r) = buttons_row {
                render_player_buttons(frame, row(r), state, hits);
            }

            if let Some(r) = progress_row {
                render_player_progress_and_volume(frame, row(r), state, progress, hits);
            }
        }
    }
    let status_row =
        Rect { y: inner.y.saturating_add(inner.height.saturating_sub(1)), height: 1, ..inner };
    render_player_status(frame, status_row, state, hits);
    if let Some(artwork) = artwork {
        artwork.render(
            frame,
            art_area,
            ArtworkPlacement::Player,
            state.playback.artwork_url.as_deref(),
            "♪",
        );
    } else {
        frame.render_widget(
            Paragraph::new("♪").centered().style(Style::default().fg(MUTED)),
            art_area,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn render_player_buttons(frame: &mut Frame<'_>, area: Rect, state: &AppState, hits: &mut HitMap) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if area.width >= 31 {
        let [_, buttons_deck, _] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(31), Constraint::Fill(1)])
                .areas(area);

        let [shuffle_area, _, prev_area, _, play_area, _, next_area, _, repeat_area] =
            Layout::horizontal([
                Constraint::Length(3), // " ⇄ "
                Constraint::Length(3), // spacing
                Constraint::Length(4), // " |◀ "
                Constraint::Length(3), // spacing
                Constraint::Length(4), // " ❚❚ " or "  ▶ "
                Constraint::Length(3), // spacing
                Constraint::Length(4), // " ▶| "
                Constraint::Length(3), // spacing
                Constraint::Length(4), // "  ↻ " or " ↻¹ "
            ])
            .areas(buttons_deck);

        let shuffle_style = if state.playback.shuffle {
            Style::default().fg(SAGE).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        frame.render_widget(Paragraph::new(" ⇄ ").style(shuffle_style), shuffle_area);
        hits.add(shuffle_area, HitTarget::PlayerShuffle);

        frame.render_widget(
            Paragraph::new(" |◀ ")
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            prev_area,
        );
        hits.add(prev_area, HitTarget::PlayerPrevious);

        let (icon, play_style) = if state.playback.playing {
            (" ❚❚ ", Style::default().fg(SAGE).add_modifier(Modifier::BOLD))
        } else {
            ("  ▶ ", Style::default().fg(PEACH).add_modifier(Modifier::BOLD))
        };
        frame.render_widget(Paragraph::new(icon).style(play_style), play_area);
        hits.add(play_area, HitTarget::PlayerToggle);

        frame.render_widget(
            Paragraph::new(" ▶| ")
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            next_area,
        );
        hits.add(next_area, HitTarget::PlayerNext);

        let (repeat_text, repeat_active) = match state.playback.repeat {
            2 => (" ↻¹ ", true),
            1 => ("  ↻ ", true),
            _ => ("  ↻ ", false),
        };
        let repeat_style = if repeat_active {
            Style::default().fg(LAVENDER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        frame.render_widget(Paragraph::new(repeat_text).style(repeat_style), repeat_area);
        hits.add(repeat_area, HitTarget::PlayerRepeat);
    } else if area.width >= 16 {
        let [_, buttons_deck, _] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(16), Constraint::Fill(1)])
                .areas(area);

        let [prev_area, _, play_area, _, next_area] = Layout::horizontal([
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Length(4),
        ])
        .areas(buttons_deck);

        frame.render_widget(
            Paragraph::new(" |◀ ")
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            prev_area,
        );
        hits.add(prev_area, HitTarget::PlayerPrevious);

        let (icon, play_style) = if state.playback.playing {
            (" ❚❚ ", Style::default().fg(SAGE).add_modifier(Modifier::BOLD))
        } else {
            ("  ▶ ", Style::default().fg(PEACH).add_modifier(Modifier::BOLD))
        };
        frame.render_widget(Paragraph::new(icon).style(play_style), play_area);
        hits.add(play_area, HitTarget::PlayerToggle);

        frame.render_widget(
            Paragraph::new(" ▶| ")
                .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            next_area,
        );
        hits.add(next_area, HitTarget::PlayerNext);
    } else {
        let (icon, play_style) = if state.playback.playing {
            ("❚❚", Style::default().fg(SAGE).add_modifier(Modifier::BOLD))
        } else {
            ("▶", Style::default().fg(PEACH).add_modifier(Modifier::BOLD))
        };
        frame.render_widget(Paragraph::new(icon).style(play_style), area);
        hits.add(area, HitTarget::PlayerToggle);
    }
}

fn render_player_progress_and_volume(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    progress: u64,
    hits: &mut HitMap,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bounded_width = area.width.min(80);
    let [_, centered_area, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(bounded_width),
        Constraint::Fill(1),
    ])
    .areas(area);

    if centered_area.width >= 60 {
        let [progress_chunk, _, volume_chunk] = Layout::horizontal([
            Constraint::Fill(1),
            Constraint::Length(2),
            Constraint::Length(18),
        ])
        .areas(centered_area);

        render_progress_bar(frame, progress_chunk, state, progress, hits, true);
        render_volume_widget(frame, volume_chunk, state.playback.volume, hits, true);
    } else if centered_area.width >= 45 {
        let [progress_chunk, _, volume_chunk] =
            Layout::horizontal([Constraint::Fill(1), Constraint::Length(2), Constraint::Length(7)])
                .areas(centered_area);

        render_progress_bar(frame, progress_chunk, state, progress, hits, false);
        render_volume_widget(frame, volume_chunk, state.playback.volume, hits, false);
    } else {
        render_progress_bar(frame, centered_area, state, progress, hits, false);
    }
}

fn render_progress_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    progress: u64,
    hits: &mut HitMap,
    show_total: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let left_text = clock(progress);
    let right_text = clock(state.playback.duration_ms);

    let (left_area, bar_area, right_area) = if show_total && area.width >= 20 {
        let [left, bar, right] =
            Layout::horizontal([Constraint::Length(6), Constraint::Fill(1), Constraint::Length(6)])
                .areas(area);
        (Some(left), bar, Some(right))
    } else if area.width >= 12 {
        let [left, bar] =
            Layout::horizontal([Constraint::Length(6), Constraint::Fill(1)]).areas(area);
        (Some(left), bar, None)
    } else {
        (None, area, None)
    };

    if let Some(left) = left_area {
        frame.render_widget(Paragraph::new(left_text).style(Style::default().fg(LAVENDER)), left);
    }
    if let Some(right) = right_area {
        frame.render_widget(
            Paragraph::new(right_text)
                .alignment(Alignment::Right)
                .style(Style::default().fg(LAVENDER)),
            right,
        );
    }

    if bar_area.width > 0 {
        if bar_area.width == 1 {
            frame.render_widget(
                Paragraph::new("●").style(Style::default().fg(Color::White)),
                bar_area,
            );
        } else {
            let track_width = bar_area.width.saturating_sub(1);
            let filled = progress
                .saturating_mul(u64::from(track_width))
                .checked_div(state.playback.duration_ms)
                .and_then(|val| usize::try_from(val).ok())
                .unwrap_or(0)
                .min(usize::from(track_width));
            let unfilled = usize::from(track_width).saturating_sub(filled);
            let spans = vec![
                Span::styled("━".repeat(filled), Style::default().fg(LAVENDER)),
                Span::styled("●", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled("─".repeat(unfilled), Style::default().fg(Color::Rgb(70, 70, 85))),
            ];
            frame.render_widget(Paragraph::new(Line::from(spans)), bar_area);
        }
        hits.add(bar_area, HitTarget::PlayerProgress);
    }
}

fn render_volume_widget(
    frame: &mut Frame<'_>,
    area: Rect,
    volume: u8,
    hits: &mut HitMap,
    full: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if full && area.width >= 18 {
        let icon = if volume == 0 {
            "🔇 "
        } else if volume < 50 {
            "🔉 "
        } else {
            "🔊 "
        };
        let icon_style =
            if volume == 0 { Style::default().fg(MUTED) } else { Style::default().fg(PEACH) };
        let segments = 8_usize;
        let filled = (usize::from(volume) * segments + 50) / 100;
        let filled = filled.min(segments);
        let unfilled = segments.saturating_sub(filled);

        let volume_line = Line::from(vec![
            Span::styled(icon, icon_style),
            Span::styled("[", Style::default().fg(MUTED)),
            Span::styled(
                "█".repeat(filled),
                Style::default().fg(if volume == 0 { MUTED } else { SAGE }),
            ),
            Span::styled("░".repeat(unfilled), Style::default().fg(Color::Rgb(70, 70, 85))),
            Span::styled("]", Style::default().fg(MUTED)),
            Span::styled(
                format!(" {volume:>3}%"),
                Style::default().fg(if volume == 0 { MUTED } else { LAVENDER }),
            ),
        ]);
        frame.render_widget(Paragraph::new(volume_line), area);
        let meter = Rect {
            x: area.x.saturating_add(3),
            width: 10.min(area.width.saturating_sub(3)),
            ..area
        };
        hits.add(meter, HitTarget::PlayerVolume);
    } else {
        let icon = if volume == 0 { "🔇" } else { "🔊" };
        let text = format!("{icon} {volume:>3}%");
        frame.render_widget(
            Paragraph::new(text).style(if volume == 0 {
                Style::default().fg(MUTED)
            } else {
                Style::default().fg(LAVENDER)
            }),
            area,
        );
        hits.add(area, HitTarget::PlayerVolume);
    }
}

fn render_player_compact_row(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &AppState,
    progress: u64,
    hits: &mut HitMap,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bounded_width = area.width.min(80);
    let [_, centered_area, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(bounded_width),
        Constraint::Fill(1),
    ])
    .areas(area);

    if centered_area.width >= 35 {
        let [play_area, _, progress_chunk, _, volume_chunk] = Layout::horizontal([
            Constraint::Length(5),
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Length(7),
        ])
        .areas(centered_area);

        let (icon, play_style) = if state.playback.playing {
            (" ❚❚ ", Style::default().fg(SAGE).add_modifier(Modifier::BOLD))
        } else {
            ("  ▶ ", Style::default().fg(PEACH).add_modifier(Modifier::BOLD))
        };
        frame.render_widget(Paragraph::new(icon).style(play_style), play_area);
        hits.add(play_area, HitTarget::PlayerToggle);

        render_progress_bar(
            frame,
            progress_chunk,
            state,
            progress,
            hits,
            progress_chunk.width >= 24,
        );
        render_volume_widget(frame, volume_chunk, state.playback.volume, hits, false);
    } else {
        render_progress_bar(frame, centered_area, state, progress, hits, false);
    }
}

fn cell_width(value: &str) -> u16 {
    u16::try_from(value.chars().count()).unwrap_or(u16::MAX)
}
fn truncate_cells(value: &str, width: u16) -> String {
    let count = usize::from(width);
    let chars = value.chars().collect::<Vec<_>>();
    if chars.len() <= count {
        return value.into();
    }
    if count <= 1 {
        return "…".chars().take(count).collect();
    }
    chars.into_iter().take(count - 1).collect::<String>() + "…"
}
#[allow(clippy::too_many_lines)]
fn render_overlay(frame: &mut Frame<'_>, state: &AppState, hits: &mut HitMap) {
    let Some(overlay) = &state.overlay else {
        return;
    };
    let area = match overlay {
        Overlay::Inspector => centered(76, 16, frame.area()),
        Overlay::Help { .. } => centered(88, 25, frame.area()),
        Overlay::Actions { actions, .. } => {
            let height = u16::try_from(actions.len().saturating_add(2)).unwrap_or(12).clamp(6, 18);
            centered(50, height, frame.area())
        }
        Overlay::PlaylistPicker { playlists, .. } => {
            let height =
                u16::try_from(playlists.len().saturating_add(2)).unwrap_or(14).clamp(8, 18);
            centered(55, height, frame.area())
        }
        Overlay::RenamePlaylist { .. } => centered(50, 7, frame.area()),
        Overlay::ShortcutCapture { .. } => centered(56, 9, frame.area()),
        _ => centered(46, 13, frame.area()),
    };
    frame.render_widget(Clear, area);
    match overlay {
        Overlay::Menu => {
            let activate = shortcut_label(state, ShortcutAction::Activate);
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let rows = AppState::NAVIGATION.iter().enumerate().map(|(index, route)| {
                hits.add(
                    Rect {
                        x: area.x + 1,
                        y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    },
                    HitTarget::Sidebar(index),
                );
                ListItem::new(format!(
                    " {} {}",
                    if index == state.sidebar.selected { "›" } else { " " },
                    route.label()
                ))
            });
            frame.render_widget(
                List::new(rows).block(focus_block(
                    format!(" Menu · {activate} to open · {cancel} to close "),
                    true,
                )),
                area,
            );
        }
        Overlay::Help { query, .. } => {
            let focus_next = shortcut_label(state, ShortcutAction::FocusNext);
            let focus_prev = shortcut_label(state, ShortcutAction::FocusPrevious);
            let move_up = shortcut_label(state, ShortcutAction::MoveUp);
            let move_down = shortcut_label(state, ShortcutAction::MoveDown);
            let page_up = shortcut_label(state, ShortcutAction::PageUp);
            let page_down = shortcut_label(state, ShortcutAction::PageDown);
            let cursor_home = shortcut_label(state, ShortcutAction::CursorHome);
            let cursor_end = shortcut_label(state, ShortcutAction::CursorEnd);
            let activate = shortcut_label(state, ShortcutAction::Activate);
            let cancel = shortcut_label(state, ShortcutAction::Cancel);

            let move_keys = if state.shortcuts.get_bindings(ShortcutAction::MoveUp).len() > 1
                && state.shortcuts.get_bindings(ShortcutAction::MoveDown).len() > 1
            {
                let up2 = state.shortcuts.get_bindings(ShortcutAction::MoveUp)[1].display_symbol();
                let down2 =
                    state.shortcuts.get_bindings(ShortcutAction::MoveDown)[1].display_symbol();
                format!("{move_up}{move_down} or {down2}/{up2}")
            } else {
                format!("{move_up}{move_down}")
            };

            let toggle_playback = shortcut_label(state, ShortcutAction::TogglePlayback);
            let prev_track = shortcut_label(state, ShortcutAction::PreviousTrack);
            let next_track = shortcut_label(state, ShortcutAction::NextTrack);
            let seek_back = shortcut_label(state, ShortcutAction::SeekBackward);
            let seek_fwd = shortcut_label(state, ShortcutAction::SeekForward);
            let vol_down = shortcut_label(state, ShortcutAction::VolumeDown);
            let vol_up = shortcut_label(state, ShortcutAction::VolumeUp);
            let shuffle = shortcut_label(state, ShortcutAction::Shuffle);
            let repeat = shortcut_label(state, ShortcutAction::Repeat);

            let home_key = shortcut_label(state, ShortcutAction::Home);
            let settings_key = shortcut_label(state, ShortcutAction::Settings);
            let queue_has_five = state
                .shortcuts
                .get_bindings(ShortcutAction::Queue)
                .iter()
                .any(|b| *b == crate::shortcuts::KeyBinding::char('5'));
            let views_end = if queue_has_five { "5" } else { &settings_key };
            let views = format!("{home_key}-{views_end} views");

            let search = shortcut_label(state, ShortcutAction::StartSearch);
            let filter = shortcut_label(state, ShortcutAction::StartFilter);
            let prev_tab = shortcut_label(state, ShortcutAction::PreviousTab);
            let next_tab = shortcut_label(state, ShortcutAction::NextTab);
            let add_queue = shortcut_label(state, ShortcutAction::AddToQueue);
            let toggle_saved = shortcut_label(state, ShortcutAction::ToggleSaved);
            let open_actions = shortcut_label(state, ShortcutAction::OpenActions);
            let detail_play = shortcut_label(state, ShortcutAction::DetailPlay);
            let detail_shuffle = shortcut_label(state, ShortcutAction::DetailShuffle);
            let detail_spotify = shortcut_label(state, ShortcutAction::DetailOpenSpotify);
            let queue = shortcut_label(state, ShortcutAction::Queue);
            let devices = shortcut_label(state, ShortcutAction::Devices);
            let inspect = shortcut_label(state, ShortcutAction::Inspect);

            let toggle_help = shortcut_label(state, ShortcutAction::ToggleHelp);
            let quit_list = shortcut_list_label(state, ShortcutAction::Quit, " · ");

            let nav_body = format!(
                "{focus_next}/{focus_prev} focus · {move_keys} select · {page_up}/{page_down} · {cursor_home}/{cursor_end} · {activate} activate · {cancel} back"
            );
            let play_body = format!(
                "{toggle_playback} play/pause · {prev_track} {next_track} previous/next · {seek_back}/{seek_fwd} seek · {vol_down}/{vol_up} volume · {shuffle} shuffle · {repeat} repeat"
            );
            let browse_body = format!(
                "{views} · {search} search · {filter} filter Library · {prev_tab}/{next_tab} filters/tabs · {add_queue} add to queue · {toggle_saved} like/unlike · {open_actions} actions · {detail_play} play · {detail_shuffle} shuffle · {detail_spotify} Spotify · {queue} Queue · {devices} devices · {inspect} inspector"
            );
            let mouse_body =
                "Click selects · double-click activates · right-click actions · wheel scrolls hovered pane"
                    .to_string();
            let help_body = format!(
                "{toggle_help} opens Help · type to filter sections · Backspace edits · {activate} stops editing · {cancel} closes"
            );
            let session_body = format!("{quit_list} quit from any screen");

            let sections = [
                ("Navigation", nav_body),
                ("Playback", play_body),
                ("Browse", browse_body),
                ("Mouse", mouse_body),
                ("Help", help_body),
                ("Session", session_body),
            ];
            let needle = query.to_lowercase();
            let lines = sections
                .into_iter()
                .filter(|(name, body)| {
                    needle.is_empty()
                        || name.to_lowercase().contains(&needle)
                        || body.to_lowercase().contains(&needle)
                })
                .flat_map(|(name, body)| {
                    [
                        Line::from(Span::styled(
                            name,
                            Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
                        )),
                        Line::from(body),
                        Line::from(""),
                    ]
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines).wrap(Wrap { trim: true }).block(focus_block(
                    format!(" Help · filter: {query} · {cancel} closes "),
                    true,
                )),
                area,
            );
        }
        Overlay::Inspector => {
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let text = state.selected_item().map_or_else(|| "No item selected".into(), |item| format!("{}\n{}\n\nType: {:?}\nURI: {}\nArtists: {}\nAlbum: {}\nDuration: {}\nAvailability: {}\n\n{}", item.title, item.subtitle, item.kind, item.uri.as_deref().unwrap_or("—"), item.artists.iter().map(|artist| artist.0.as_str()).collect::<Vec<_>>().join(", "), item.album.as_ref().map_or("—", |album| album.0.as_str()), item.duration_ms.map_or_else(|| "—".into(), clock), if item.available { "available" } else { "unavailable" }, item.metadata));
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: true })
                    .block(focus_block(format!(" Inspector · {cancel} closes "), true)),
                area,
            );
        }
        Overlay::Actions { selected, actions } => {
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let rows = actions.iter().enumerate().map(|(index, action)| {
                hits.add(
                    Rect {
                        x: area.x + 1,
                        y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                        width: area.width.saturating_sub(2),
                        height: 1,
                    },
                    HitTarget::ActionRow(index),
                );
                ListItem::new(format!(
                    " {} {}",
                    if index == *selected { "›" } else { " " },
                    action.label()
                ))
            });
            frame.render_widget(
                List::new(rows).block(focus_block(format!(" Actions · {cancel} closes "), true)),
                area,
            );
        }
        Overlay::PlaylistPicker { selected, playlists, .. } => {
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let activate = shortcut_label(state, ShortcutAction::Activate);
            if playlists.is_empty() {
                frame.render_widget(
                    Paragraph::new(" Loading playlists… ")
                        .block(focus_block(format!(" Add to Playlist · {cancel} closes "), true)),
                    area,
                );
            } else {
                let rows = playlists.iter().enumerate().map(|(index, playlist)| {
                    hits.add(
                        Rect {
                            x: area.x + 1,
                            y: area.y + 1 + u16::try_from(index).unwrap_or(u16::MAX),
                            width: area.width.saturating_sub(2),
                            height: 1,
                        },
                        HitTarget::PlaylistPickerRow(index),
                    );
                    ListItem::new(format!(
                        " {} {}",
                        if index == *selected { "›" } else { " " },
                        playlist.title
                    ))
                });
                frame.render_widget(
                    List::new(rows).block(focus_block(
                        format!(" Add to Playlist · {activate} selects · {cancel} closes "),
                        true,
                    )),
                    area,
                );
            }
        }
        Overlay::RenamePlaylist { name, .. } => {
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let activate = shortcut_label(state, ShortcutAction::Activate);
            let input_text = format!(" {name}█");
            let p = Paragraph::new(vec![
                Line::from(Span::styled(" Enter new playlist name:", Style::default().fg(MUTED))),
                Line::from(""),
                Line::from(Span::styled(
                    input_text,
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
            ])
            .block(focus_block(
                format!(" Rename Playlist · {activate} submits · {cancel} cancels "),
                true,
            ));
            frame.render_widget(p, area);
        }
        Overlay::ShortcutCapture { action, conflict: Some((key, conflicting_action)) } => {
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let activate = shortcut_label(state, ShortcutAction::Activate);
            let lines = vec![
                Line::from(Span::styled(
                    format!(" Key '{}' is already bound to:", key.to_canonical()),
                    Style::default().fg(MUTED),
                )),
                Line::from(Span::styled(
                    format!("   {}", conflicting_action.display_name()),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" Reassign to {}?", action.display_name()),
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" {activate} reassigns · {cancel} cancels"),
                    Style::default().fg(PEACH),
                )),
            ];
            frame.render_widget(
                Paragraph::new(lines).block(focus_block(" Shortcut Conflict ", true)),
                area,
            );
        }
        Overlay::ShortcutCapture { action, conflict: None } => {
            let cancel = shortcut_label(state, ShortcutAction::Cancel);
            let current = state.shortcuts.list_label(*action, ", ");
            let lines = vec![
                Line::from(Span::styled(
                    format!(" Action: {}", action.display_name()),
                    Style::default().fg(PEACH).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!(" Current: {current}"),
                    Style::default().fg(MUTED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    " Press a key combination to assign…",
                    Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    format!(" {cancel} cancels · Delete unbinds"),
                    Style::default().fg(MUTED),
                )),
            ];
            frame.render_widget(
                Paragraph::new(lines).block(focus_block(" Assign Shortcut ", true)),
                area,
            );
        }
        Overlay::DevicePicker => frame.render_widget(
            Paragraph::new(
                "No active device. Open Spotify on a device, then choose it from Devices.",
            )
            .wrap(Wrap { trim: true })
            .block(focus_block(" Choose a device ", true)),
            area,
        ),
    }
}

fn centered(width_percent: u16, height: u16, area: Rect) -> Rect {
    let width = area
        .width
        .saturating_mul(width_percent)
        .checked_div(100)
        .unwrap_or(area.width)
        .max(20)
        .min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn clock(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BrowseItem, PageState, Section};
    #[test]
    fn player_regions_are_distinct_and_metadata_is_not_duplicated() {
        let backend = TestBackend::new(120, 35);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.playback.title = "Title only".into();
        state.playback.artist = "Artist alone".into();
        state.playback.album = "Album alone".into();
        state.playback.duration_ms = 143_000;
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let title_row = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .skip(25_usize * 120)
            .take(120)
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(title_row.contains("Title only"));
        assert!(!title_row.contains("Artist alone") && !title_row.contains("Album alone"));
        let progress = hits.rect_for(&HitTarget::PlayerProgress).unwrap();
        let volume = hits.rect_for(&HitTarget::PlayerVolume).unwrap();
        assert!(progress.x + progress.width <= volume.x || volume.x + volume.width <= progress.x);
        assert_ne!(hits.rect_for(&HitTarget::PlayerDevice), hits.rect_for(&HitTarget::Help));
        assert_ne!(
            hits.rect_for(&HitTarget::PlayerPrevious),
            hits.rect_for(&HitTarget::PlayerNext)
        );
        assert_ne!(
            hits.rect_for(&HitTarget::PlayerShuffle),
            hits.rect_for(&HitTarget::PlayerRepeat)
        );
    }

    use ratatui::{Terminal, backend::TestBackend};

    fn draw(width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page.state = LoadState::Empty("No albums saved".into());
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>()
    }

    #[test]
    fn supported_sizes_render_without_panicking() {
        for (width, height) in [(80, 24), (120, 35), (140, 35), (160, 45)] {
            let output = draw(width, height);
            assert!(
                output.contains("lspotify")
                    || output.contains("Navigation")
                    || output.contains("Menu")
            );
        }
    }

    #[test]
    fn undersized_terminal_keeps_quit_instruction() {
        assert!(draw(79, 20).contains("Ctrl+C"));
    }

    #[test]
    fn wide_layout_exposes_queue_hit_region() {
        let backend = TestBackend::new(160, 45);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        assert!(hits.queue.is_some());
    }

    #[test]
    fn queue_grows_with_a_wider_terminal() {
        let queue_width = |width| {
            let backend = TestBackend::new(width, 45);
            let mut terminal = Terminal::new(backend).unwrap();
            let mut state = AppState::default();
            let mut hits = HitMap::default();
            terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
            hits.queue.unwrap().width
        };
        assert!(queue_width(160) > queue_width(140));
    }

    #[test]
    fn long_unicode_and_error_state_render() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page.title = "夢の中で café com açúcar — очень длинное имя".into();
        state.page.state = LoadState::Failed("Network unavailable".into());
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(output.contains("Network unavailable"));
    }

    #[test]
    fn compact_playback_controls_bounded_and_centered_on_wide_screen() {
        let backend = TestBackend::new(160, 45);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.playback.title = "Wide Screen Track".into();
        state.playback.duration_ms = 200_000;
        state.playback.volume = 80;
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        let progress = hits.rect_for(&HitTarget::PlayerProgress).unwrap();
        let volume = hits.rect_for(&HitTarget::PlayerVolume).unwrap();
        let prev = hits.rect_for(&HitTarget::PlayerPrevious).unwrap();
        let toggle = hits.rect_for(&HitTarget::PlayerToggle).unwrap();
        let next = hits.rect_for(&HitTarget::PlayerNext).unwrap();
        let shuffle = hits.rect_for(&HitTarget::PlayerShuffle).unwrap();
        let repeat = hits.rect_for(&HitTarget::PlayerRepeat).unwrap();

        // Check that buttons are centered and bounded
        assert!(shuffle.x < prev.x);
        assert!(prev.x < toggle.x);
        assert!(toggle.x < next.x);
        assert!(next.x < repeat.x);
        assert!(repeat.x.saturating_add(repeat.width).saturating_sub(shuffle.x) <= 34);

        // Check that progress and volume are on the same row and bounded
        assert_eq!(progress.y, volume.y);
        assert!(progress.x + progress.width <= volume.x);
        // Controls do not stretch to the entire width of 160
        assert!(volume.x.saturating_add(volume.width).saturating_sub(progress.x) <= 80);
    }

    #[test]
    fn side_by_side_progress_and_volume_rendering() {
        let backend = TestBackend::new(120, 35);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.playback.title = "Test Song".into();
        state.playback.duration_ms = 240_000; // 4 minutes
        state.playback.playing = true;
        state.playback.volume = 50;
        state.playback.shuffle = true;
        state.playback.repeat = 2; // track repeat
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        // Check for sleek progress bar scrubber knob
        assert!(output.contains('●'));
        // Check for volume bar brackets and blocks
        assert!(output.contains("50%"));
        // Check for pause bars
        assert!(output.contains("❚❚"));
        // Check for plain shuffle, repeat, and prev/next glyphs
        assert!(output.contains('⇄'));
        assert!(output.contains("↻¹"));
        assert!(output.contains("|◀"));
        assert!(output.contains("▶|"));
    }

    #[test]
    fn responsive_narrow_terminal_player_rendering() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.playback.title = "Narrow Display Song".into();
        state.playback.duration_ms = 180_000;
        state.playback.volume = 75;
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        let progress = hits.rect_for(&HitTarget::PlayerProgress).unwrap();
        let volume = hits.rect_for(&HitTarget::PlayerVolume).unwrap();
        assert_eq!(progress.y, volume.y);
        assert!(progress.x + progress.width <= volume.x);
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn detail_page_header_renders_buttons_and_eliminates_duplicated_info() {
        let backend = TestBackend::new(120, 35);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page = PageState::loading(
            Route::Album { uri: "spotify:album:1".into(), title: "OK Computer".into() },
            1,
        );
        state.page.title = "OK Computer".into();
        state.page.subtitle = "by Radiohead · 1997 · 12 tracks".into();
        state.page.uri = Some("spotify:album:1".into());
        state.page.external_url = Some("https://open.spotify.com/album/1".into());
        state.page.state = LoadState::Ready;
        state.page.sections = vec![Section {
            title: "Tracks".into(),
            items: vec![BrowseItem {
                id: "track-1".into(),
                kind: EntityKind::Track,
                title: "Airbag".into(),
                subtitle: "Radiohead".into(),
                metadata: String::new(),
                uri: Some("spotify:track:1".into()),
                external_url: None,
                artwork_url: None,
                artists: vec![],
                album: None,
                duration_ms: Some(284_000),
                available: true,
                saved: None,
                context: None,
                restricted: false,
            }],
        }];

        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        // 1. Verify HitTargets for the action buttons were added
        let play_hit = hits.rect_for(&HitTarget::DetailPlay);
        let shuffle_hit = hits.rect_for(&HitTarget::DetailShuffle);
        let spotify_hit = hits.rect_for(&HitTarget::DetailOpenSpotify);
        assert!(play_hit.is_some(), "DetailPlay hit target missing");
        assert!(shuffle_hit.is_some(), "DetailShuffle hit target missing");
        assert!(spotify_hit.is_some(), "DetailOpenSpotify hit target missing");

        let play_rect = play_hit.unwrap();
        let shuffle_rect = shuffle_hit.unwrap();
        let spotify_rect = spotify_hit.unwrap();
        assert_eq!(play_rect.y, shuffle_rect.y);
        assert_eq!(shuffle_rect.y, spotify_rect.y);
        assert_eq!(play_rect.x + play_rect.width + 2, shuffle_rect.x);
        assert_eq!(shuffle_rect.x + shuffle_rect.width + 2, spotify_rect.x);

        let buf = terminal.backend().buffer();
        // Verify exact cell alignment for button boundaries
        assert_eq!(buf[(play_rect.x, play_rect.y)].symbol(), "[");
        assert_eq!(buf[(play_rect.x + play_rect.width - 1, play_rect.y)].symbol(), "]");
        assert_eq!(buf[(play_rect.x + play_rect.width, play_rect.y)].symbol(), " ");
        assert_eq!(buf[(shuffle_rect.x, shuffle_rect.y)].symbol(), "[");
        assert_eq!(buf[(shuffle_rect.x + shuffle_rect.width - 1, shuffle_rect.y)].symbol(), "]");
        assert_eq!(buf[(shuffle_rect.x + shuffle_rect.width, shuffle_rect.y)].symbol(), " ");
        assert_eq!(buf[(spotify_rect.x, spotify_rect.y)].symbol(), "[");
        assert_eq!(buf[(spotify_rect.x + spotify_rect.width - 1, spotify_rect.y)].symbol(), "]");

        // 2. Verify buffer text & de-duplication
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(output.contains("Play album"));
        assert!(output.contains("Play with shuffle"));
        assert!(output.contains("Open in Spotify"));
        assert!(output.contains("Tracks"));
        assert!(output.contains("Airbag"));
    }

    #[test]
    fn detail_page_long_title_preserves_button_geometry() {
        let backend = TestBackend::new(80, 35);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        let long_title =
            "A Very Long Album Title That Exceeds Normal Screen Width And Would Wrap In Paragraph"
                .to_string();
        state.page = PageState::loading(
            Route::Album { uri: "spotify:album:long".into(), title: long_title.clone() },
            1,
        );
        state.page.title = long_title;
        state.page.subtitle = "by Pink Floyd · 1973 · 10 tracks".into();
        state.page.state = LoadState::Ready;

        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        let play_hit = hits.rect_for(&HitTarget::DetailPlay);
        let shuffle_hit = hits.rect_for(&HitTarget::DetailShuffle);
        let spotify_hit = hits.rect_for(&HitTarget::DetailOpenSpotify);
        assert!(play_hit.is_some());
        assert!(shuffle_hit.is_some());
        assert!(spotify_hit.is_some());

        let play_rect = play_hit.unwrap();
        let buf = terminal.backend().buffer();
        // Button row must strictly remain on row 6 (area.y(2) + 4) despite very long title
        assert_eq!(play_rect.y, 6);
        assert_eq!(buf[(play_rect.x, play_rect.y)].symbol(), "[");
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn artist_detail_header_renders_songs_and_artist_play_labels() {
        let backend = TestBackend::new(120, 35);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page = PageState::loading(
            Route::Artist { uri: "spotify:artist:radiohead".into(), title: "Radiohead".into() },
            1,
        );
        state.page.title = "Radiohead".into();
        state.page.subtitle = "50 tracks".into();
        state.page.state = LoadState::Ready;

        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(output.contains("Play artist"));
        assert!(output.contains("Play with shuffle"));
        assert!(output.contains("Open in Spotify"));
        assert!(output.contains("Songs"));
    }

    #[test]
    fn side_player_rendering_and_topbar_queue() {
        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.playback.title = "Side Track".into();
        state.playback.artist = "Side Artist".into();
        state.playback.album = "Side Album".into();
        state.playback.duration_ms = 180_000;
        state.playback.volume = 70;
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        assert_eq!(state.layout, LayoutMode::SidePlayer);
        // Topbar has [5] Queue in SidePlayer
        assert!(hits.rect_for(&HitTarget::TopNav(Route::Queue)).is_some());
        // Inline queue pane is omitted
        assert!(hits.queue.is_none());

        // Player is rendered in the right pane: x should be >= 90
        let toggle = hits.rect_for(&HitTarget::PlayerToggle).unwrap();
        assert!(toggle.x >= 90);
        let progress = hits.rect_for(&HitTarget::PlayerProgress).unwrap();
        assert!(progress.x >= 90);
        let volume = hits.rect_for(&HitTarget::PlayerVolume).unwrap();
        assert!(volume.x >= 90);

        // In vertical deck, toggle button is vertically above progress, which is above volume
        assert!(toggle.y < progress.y);
        assert!(progress.y <= volume.y);
    }

    #[test]
    fn stacked_queue_rendering_and_collapsing() {
        let backend = TestBackend::new(100, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.queue_upcoming.push(BrowseItem {
            id: "q1".into(),
            kind: EntityKind::Track,
            title: "Upcoming Song".into(),
            subtitle: "Upcoming Artist".into(),
            metadata: String::new(),
            uri: Some("spotify:track:q1".into()),
            external_url: None,
            artwork_url: None,
            artists: Vec::new(),
            album: None,
            duration_ms: Some(180_000),
            available: true,
            saved: None,
            context: None,
            restricted: false,
        });
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        assert_eq!(state.layout, LayoutMode::StackedQueue);
        // Inline queue is visible
        assert!(hits.queue.is_some());
        let queue_rect = hits.queue.unwrap();
        // Player is bottom 11 rows, so y should be >= 31
        let player_toggle = hits.rect_for(&HitTarget::PlayerToggle).unwrap();
        assert!(queue_rect.y + queue_rect.height <= player_toggle.y);
        // Topbar does NOT contain [5] Queue because inline queue is present
        assert!(hits.rect_for(&HitTarget::TopNav(Route::Queue)).is_none());

        // When navigating to Route::Queue, inline queue collapses
        state.page.route = Route::Queue;
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        assert!(hits.queue.is_none());
    }

    #[test]
    fn responsive_detail_header_preserves_tracks_on_compact_screens() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.page.route =
            Route::Album { uri: "spotify:album:test".into(), title: "Short Album".into() };
        for i in 0..10 {
            state.page.sections.push(Section {
                title: format!("Disc {i}"),
                items: vec![BrowseItem {
                    id: format!("t{i}"),
                    kind: EntityKind::Track,
                    title: format!("Track {i}"),
                    subtitle: "Artist".into(),
                    metadata: String::new(),
                    uri: Some(format!("spotify:track:{i}")),
                    external_url: None,
                    artwork_url: None,
                    artists: Vec::new(),
                    album: None,
                    duration_ms: Some(180_000),
                    available: true,
                    saved: None,
                    context: None,
                    restricted: false,
                }],
            });
        }
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        // On 80x24 (Compact), body height is 11 (< 16), so header height is 5.
        // List area has 11 - 5 = 6 rows, so content_height = 4 visible track rows!
        assert!(state.content_height >= 4);
        assert!(hits.rect_for(&HitTarget::ContentRow(0)).is_some());
        assert!(hits.rect_for(&HitTarget::ContentRow(1)).is_some());
    }

    #[test]
    fn topbar_80_col_alignment_and_hit_targets() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        // 80x24 is Compact: has 5 tabs
        let home = hits.rect_for(&HitTarget::TopNav(Route::Home)).unwrap();
        assert_eq!(home.x, 3); // " ♪ " is 3 cells
        let queue = hits.rect_for(&HitTarget::TopNav(Route::Queue)).unwrap();
        assert!(queue.x + queue.width <= 80);
    }

    #[test]
    fn side_player_status_uses_ready_and_renders_separator() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(output.contains("Ready"));
        assert!(!output.contains("lspot…"));
        assert!(output.contains("·"));
        assert!(hits.rect_for(&HitTarget::PlayerDevice).is_some());
    }

    #[test]
    fn side_player_safely_handles_small_player_height() {
        let backend = TestBackend::new(100, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = AppState { layout: LayoutMode::SidePlayer, ..Default::default() };
        let mut hits = HitMap::default();
        // Area with height 10 and width 38
        terminal
            .draw(|frame| {
                render_player(
                    frame,
                    Rect { x: 62, y: 2, width: 38, height: 10 },
                    &state,
                    &mut hits,
                    None,
                );
            })
            .unwrap();
        assert!(hits.rect_for(&HitTarget::PlayerDevice).is_some());
    }

    #[test]
    fn help_overlay_shows_five_views() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState {
            overlay: Some(crate::state::Overlay::Help { query: String::new(), editing: false }),
            ..Default::default()
        };
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(output.contains("1-5 views"));
    }

    #[test]
    fn buttons_breakpoint_at_30_vs_31() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = AppState::default();

        // At width 30: 3 buttons (prev, play, next), no shuffle or repeat
        let mut hits = HitMap::default();
        terminal
            .draw(|frame| {
                render_player_buttons(
                    frame,
                    Rect { x: 0, y: 0, width: 30, height: 1 },
                    &state,
                    &mut hits,
                );
            })
            .unwrap();
        assert!(hits.rect_for(&HitTarget::PlayerToggle).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerPrevious).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerNext).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerShuffle).is_none());
        assert!(hits.rect_for(&HitTarget::PlayerRepeat).is_none());

        // At width 31: all 5 buttons
        let mut hits = HitMap::default();
        terminal
            .draw(|frame| {
                render_player_buttons(
                    frame,
                    Rect { x: 0, y: 0, width: 31, height: 1 },
                    &state,
                    &mut hits,
                );
            })
            .unwrap();
        assert!(hits.rect_for(&HitTarget::PlayerToggle).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerPrevious).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerNext).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerShuffle).is_some());
        assert!(hits.rect_for(&HitTarget::PlayerRepeat).is_some());
    }

    #[test]
    fn side_player_track_avatar_is_centered() {
        let backend = TestBackend::new(140, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();
        state.playback.title = "Side Track".into();
        state.playback.artist = "Side Artist".into();
        state.playback.album = "Side Album".into();
        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();

        let buffer = terminal.backend().buffer();
        // In 140x24: body width is 140. Player width is 44 (clamped).
        // Player pane area: x = 96, width = 44. Inner area: x = 97, width = 42.
        // Center of inner player pane is 97 + 21 = 118.
        let mut found_symbol = false;
        for y in 2..15 {
            for x in 96..140 {
                let symbol = buffer[(x, y)].symbol();
                if symbol == "♪" {
                    assert!(
                        (116..=120).contains(&x),
                        "Avatar placeholder at (x={x}, y={y}) is not centered in player inner area (97..139, center 118)"
                    );
                    found_symbol = true;
                }
            }
        }
        assert!(found_symbol, "Expected placeholder ♪ to be rendered in deck");
    }

    #[test]
    fn artwork_under_overlays_visibility() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState {
            overlay: Some(crate::state::Overlay::Help { query: String::new(), editing: false }),
            artwork_under_overlays: false,
            playback: crate::state::PlaybackState {
                artwork_url: Some("https://example.com/art.jpg".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut manager = ArtworkManager::new(lspotify_core::CliArtworkPreference::Auto);
        manager.set_placement_for_test(ArtworkPlacement::Player, "https://example.com/art.jpg");
        assert!(manager.has_placement(ArtworkPlacement::Player));

        let mut hits = HitMap::default();
        terminal
            .draw(|frame| render_with_artwork(frame, &mut state, &mut hits, &mut manager))
            .unwrap();

        // Default setting hides artwork under overlays (placement is cleared)
        assert!(!manager.has_placement(ArtworkPlacement::Player));

        // When artwork_under_overlays is enabled, placement is preserved under overlays
        manager.set_placement_for_test(ArtworkPlacement::Player, "https://example.com/art.jpg");
        state.artwork_under_overlays = true;
        terminal
            .draw(|frame| render_with_artwork(frame, &mut state, &mut hits, &mut manager))
            .unwrap();

        assert!(manager.has_placement(ArtworkPlacement::Player));
    }

    #[test]
    fn dynamic_shortcut_labels_reflect_reconfigured_keys() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState {
            overlay: Some(crate::state::Overlay::Help { query: String::new(), editing: false }),
            ..Default::default()
        };

        // Reconfigure Play/Pause from Space to p, and Quit to F10
        state.shortcuts.force_assign(
            ShortcutAction::TogglePlayback,
            crate::shortcuts::KeyBinding::parse("p").unwrap(),
        );
        state.shortcuts.force_assign(
            ShortcutAction::Quit,
            crate::shortcuts::KeyBinding::parse("Ctrl+Q").unwrap(),
        );

        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(output.contains("p play/pause"));
        assert!(!output.contains("Space play/pause"));
        assert!(output.contains("Ctrl+Q quit from any screen"));
    }

    #[test]
    fn topbar_reflects_reconfigured_navigation_shortcuts() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = AppState::default();

        state
            .shortcuts
            .force_assign(ShortcutAction::Home, crate::shortcuts::KeyBinding::parse("h").unwrap());
        state.shortcuts.force_assign(
            ShortcutAction::Search,
            crate::shortcuts::KeyBinding::parse("s").unwrap(),
        );

        let mut hits = HitMap::default();
        terminal.draw(|frame| render(frame, &mut state, &mut hits)).unwrap();
        let output = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();

        assert!(output.contains("[h] ⌂ Home"));
        assert!(output.contains("[s] ⌕ Search"));
    }
}
