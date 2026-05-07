# Theme Creation Guide

This directory contains theme files for the Rust Server Controller application. You can create your own custom themes by creating new JSON files in this directory.

## Theme File Format

Theme files are JSON files that define colors for the UI. Each theme should have the following structure:

```json
{
  "name": "My Custom Theme",
  "color_space": "Oklch",
  "bg_dark": { "l": 0.1, "c": 0.05, "h": 290.0, "a": 1.0 },
  "bg": { "l": 0.15, "c": 0.05, "h": 290.0, "a": 1.0 },
  "bg_light": { "l": 0.2, "c": 0.05, "h": 290.0, "a": 1.0 },
  "text": { "l": 0.96, "c": 0.02, "h": 290.0, "a": 1.0 },
  "text_muted": { "l": 0.76, "c": 0.02, "h": 290.0, "a": 1.0 },
  "highlight": { "l": 0.5, "c": 0.2, "h": 290.0, "a": 1.0 },
  "border": { "l": 0.4, "c": 0.1, "h": 290.0, "a": 1.0 },
  "border_muted": { "l": 0.3, "c": 0.05, "h": 290.0, "a": 1.0 },
  "primary": { "l": 0.7, "c": 0.25, "h": 290.0, "a": 1.0 },
  "secondary": { "l": 0.7, "c": 0.2, "h": 250.0, "a": 1.0 },
  "danger": { "l": 0.7, "c": 0.2, "h": 30.0, "a": 1.0 },
  "warning": { "l": 0.7, "c": 0.2, "h": 100.0, "a": 1.0 },
  "success": { "l": 0.7, "c": 0.2, "h": 140.0, "a": 1.0 },
  "info": { "l": 0.7, "c": 0.2, "h": 260.0, "a": 1.0 }
}
```

## Color Properties

- `name`: The display name of the theme (shown in the theme selector)
- `color_space`: The color space used (Oklch is recommended)
- Color values use the following properties:
  - `l`: Lightness (0 to 1)
  - `c`: Chroma/saturation (0 to 0.4 is a good range)
  - `h`: Hue angle in degrees (0 to 360)
  - `a`: Alpha/opacity (0 to 1)

## Color Meanings

- `bg_dark`, `bg`, `bg_light`: Background colors (dark to light)
- `text`, `text_muted`: Text colors (primary and secondary)
- `highlight`: Used for UI highlights
- `border`, `border_muted`: Border colors
- `primary`, `secondary`: Primary and secondary brand colors
- `danger`, `warning`, `success`, `info`: Semantic colors for feedback

## Tips

- Study the examples in this directory to understand how colors work together
- The default themes are good starting points
- Try small changes first to understand how they affect the UI
- Use Oklch color space for better perceptual uniformity
- Keep contrast ratios high for better accessibility
