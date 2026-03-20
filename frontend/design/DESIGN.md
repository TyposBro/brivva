# Design System Document

## 1. Overview & Creative North Star: "The Kinetic Monolith"

This design system is built for high-performance impact. Our Creative North Star is **The Kinetic Monolith**—a visual philosophy that treats digital space as a physical, architectural environment. Unlike standard web layouts that rely on thin lines and predictable grids, this system uses massive typographic weight, high-contrast tonal shifts, and aggressive negative space to command attention.

We break the "template" look by favoring intentional asymmetry. Content isn't just "placed"; it is carved out of a deep, dark environment. By layering varied shades of obsidian and charcoal with "laser-cut" vibrant blue accents, we create a high-tech, professional atmosphere that feels both authoritative and futuristic.

---

## 2. Colors: Tonal Depth & Radiant Accents

Our palette is rooted in the depth of `background: #131313`. We do not use color to decorate; we use it to direct the eye and define space.

### The "No-Line" Rule
**Explicit Instruction:** 1px solid borders for sectioning are strictly prohibited. The UI must feel like a continuous flow of carved surfaces. Separate logical sections using background shifts (e.g., a `surface_container_low` section transitioning into a `surface` section) or large gaps of vertical space from our Spacing Scale.

### Surface Hierarchy & Nesting
Treat the interface as a series of nested physical layers. 
- Use `surface_container_lowest` (#0E0E0E) for the deepest recesses (like background sections).
- Use `surface_container_high` (#2A2A2A) for interactive elements like cards.
- **The Glass & Gradient Rule:** For primary CTAs or high-impact hero sections, use a linear gradient transitioning from `primary_container` (#2563EB) to `secondary_container` (#7C03D3) at a 135-degree angle. This adds a "digital soul" that flat colors cannot replicate.

### Core Tokens
*   **Primary (Action):** `primary` (#B4C5FF) for interactive highlights.
*   **Primary Container (Brand Force):** `primary_container` (#2563EB) for high-impact backgrounds.
*   **Surface:** `surface` (#131313) the foundation of the experience.
*   **On-Surface:** `on_surface` (#E5E2E1) for maximum readability.

---

## 3. Typography: The Brutalist Voice

Typography is our primary design element. We pair the industrial, condensed power of **Space Grotesk** (Display) with the clean, technical precision of **Inter**.

*   **Display (Space Grotesk):** Set in high-weight bold. Use `display-lg` (3.5rem) for hero statements. Tighten letter-spacing (-0.05em) to create the "Monolith" effect.
*   **Headline (Space Grotesk):** Use for section headers. These should feel like architectural labels—unapologetic and clear.
*   **Body (Inter):** Reserved for technical details and narrative text. Use `body-md` (0.875rem) with generous line-height (1.6) to provide a "breathing" contrast to the heavy headers.
*   **Label (Inter Monospace):** Use `ui-monospace` for data points, "The Core Engine" callouts, and technical metadata. This reinforces the high-tech, engineered nature of the product.

---

## 4. Elevation & Depth: Tonal Layering

We reject traditional shadows in favor of **Tonal Layering**. Depth is achieved through the physical stacking of different surface-container tiers.

*   **The Layering Principle:** Place a `surface_container_highest` (#353534) component on top of a `surface_container_low` (#1C1B1B) section. This creates a natural, sophisticated lift.
*   **Ambient Shadows:** If a floating element (like a Modal) is required, use a shadow with a 40px blur at 6% opacity, tinted with `#000000`. It should feel like an ambient occlusion, not a drop shadow.
*   **Glassmorphism:** Use `surface_bright` (#3A3939) at 60% opacity with a 12px backdrop-blur for navigation bars or floating tooltips. This integrates the component into the environment rather than "sticking" it on top.
*   **Ghost Borders:** If an accessibility requirement demands a border, use `outline_variant` (#434655) at 15% opacity. It must be felt, not seen.

---

## 5. Components: Precision Engineered

### Buttons
*   **Primary:** High-weight, 0.25rem (`DEFAULT`) roundedness. Background: `primary_container`. Text: `on_primary_container`. No border.
*   **Tertiary/Ghost:** Text-only in `primary`. On hover, add a subtle `surface_container_highest` background shift.

### Cards & Sections
*   **No Dividers:** Forbid the use of line-dividers. Use 4rem (`16`) or 6rem (`24`) spacing to separate content blocks. 
*   **Layout:** Use asymmetrical padding. A card might have a large `12` (3rem) padding on the left and a tight `4` (1rem) on the top to create a modern, editorial feel.

### Input Fields
*   **State:** Default state uses `surface_container_highest`. Focused state adds a `primary` glow (2px outer blur) and transitions the background to `surface_bright`.
*   **Typography:** Labels must use `label-md` in `on_surface_variant` (#C3C6D7).

### Signature Component: The "Data Matrix"
A specific table or list format for technical specs. Forgo all horizontal lines. Use `surface_container_low` for the header and `surface_container_lowest` for alternating rows (zebra striping) to define the grid without using "ink."

---

## 6. Do’s and Don’ts

### Do:
*   **DO** use extreme typographic scale. Make the difference between your Display and Body text dramatic.
*   **DO** use "True Black" (#0F0F0F) for deep backgrounds to make the vibrant Blue (#2563EB) pop with electric intensity.
*   **DO** lean into whitespace. Professionalism is signaled by the luxury of unused space.

### Don't:
*   **DON'T** use 100% white (#FFFFFF) for body text; use `on_surface` (#E5E2E1) to reduce eye strain on dark backgrounds.
*   **DON'T** use rounded corners larger than `xl` (0.75rem). This system is "High-Tech" and "Professional," not "Playful."
*   **DON'T** use standard shadows. If it looks like a default Material Design shadow, it is wrong for this system.