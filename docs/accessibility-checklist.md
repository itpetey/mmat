# MMAT Workbench Accessibility Checklist

Manual verification items for the web workbench (`/`).

## Keyboard Navigation

- [ ] Tab order reaches all interactive controls: lane navigation, chat composer, notifications, view controls, run controls, and inline action buttons
- [ ] Shift+Tab reverses tab order through all controls
- [ ] Enter and Space activate buttons (view toggles, run controls, reset)
- [ ] Enter submits the chat composer form
- [ ] Escape closes the notification panel when open
- [ ] Arrow keys navigate DAG steps in the DAG view (future enhancement)
- [ ] Arrow keys navigate event rows in the events view (future enhancement)

## Focus Indicators

- [ ] All interactive elements show visible `:focus-visible` outlines (green highlight)
- [ ] Focus is not trapped inside any component
- [ ] View switches retain sensible focus (chat → DAG → events)
- [ ] Notification panel toggle shows focus outline

## Screen Reader Support

- [ ] `aria-label` present on all icon-only controls (view buttons, run controls)
- [ ] `aria-label` present on the chat textarea
- [ ] `aria-label` present on the run selector
- [ ] `role="log"` on the conversation channel announces new messages
- [ ] `role="feed"` on the events list announces dynamic content
- [ ] `role="status"` and `aria-live="polite"` on next-action summary
- [ ] `role="status"` and `aria-live="polite"` on connection status bar
- [ ] `role="alertdialog"` on notification panel
- [ ] `role="list"` on DAG canvas
- [ ] `aria-selected` updates on active event rows
- [ ] SVG icons use `aria-hidden="true"`
- [ ] Notification count badge has `role="button"` and `aria-label`

## Semantic HTML

- [ ] `<header>` contains topbar navigation
- [ ] `<main>` wraps the workspace area
- [ ] `<section>` wraps each view (chat, DAG, events)
- [ ] `<aside>` wraps side panels (step detail, event detail)
- [ ] `<form>` wraps the chat composer
- [ ] `<button>` elements use `type="button"` to prevent form submission
- [ ] `<html lang="en">` set correctly

## Responsive Layout

- [ ] At viewports ≤ 920px: DAG and detail panes stack vertically
- [ ] At viewports ≤ 920px: chat layout adjusts (smaller speaker column, no horizontal overflow)
- [ ] At viewports ≤ 920px: DAG canvas collapses to single column
- [ ] At viewports ≤ 920px: all interactive controls remain tappable (min 44px touch targets)
- [ ] No horizontal scroll at any supported viewport width

## Colour And Contrast

- [ ] Success indicators (green) have sufficient contrast against dark background
- [ ] Error/danger indicators (red) have sufficient contrast against dark background
- [ ] Warning/reconnecting state (yellow) has sufficient contrast
- [ ] Text remains readable when connection status bar animates
- [ ] No information is conveyed by colour alone (status dots always paired with text)

## Dynamic Content

- [ ] Connection status bar announces changes via `aria-live`
- [ ] Next-action summary updates and announces via `aria-live`
- [ ] Notification count updates when new notifications arrive
- [ ] Chat auto-scrolls to bottom but does not trap focus
- [ ] Code blocks render without executing embedded HTML/scripts

## States

- [ ] Welcome/prompt state visible when no conversation history exists
- [ ] Action request pending state visible with actionable prompt
- [ ] Running state indicators (pulsing animation) do not cause seizures (≤ 3 flashes/second)
- [ ] Failed/error state visible with red indicators and error banners
- [ ] Reconnecting state visible with yellow status bar
- [ ] Disconnected state visible with red status bar
- [ ] Empty states display meaningful messages (no events, no DAG steps, no roles)
- [ ] Error states display error content with context (missing artefacts, missing paths)

## Verification Date

Fill in after manual review:

- Date: _______________
- Reviewer: _______________
- Browser(s): _______________
- Screen reader(s): _______________
- Notes: _______________
