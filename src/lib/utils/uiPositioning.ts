export interface ContextMenuPlacementInput {
  pointerX: number;
  pointerY: number;
  menuWidth: number;
  menuHeight: number;
  viewportWidth: number;
  viewportHeight: number;
  viewportPadding?: number;
  minMenuHeight?: number;
}

export interface ContextMenuPlacement {
  left: number;
  top: number;
  maxHeight: number;
}

/**
 * Compute a viewport-safe placement for pointer-triggered context menus.
 */
export function computeContextMenuPlacement(input: ContextMenuPlacementInput): ContextMenuPlacement {
  const padding = input.viewportPadding ?? 8;
  const minMenuHeight = input.minMenuHeight ?? 140;
  const maxHeight = Math.max(minMenuHeight, input.viewportHeight - padding * 2);
  const renderedHeight = Math.min(input.menuHeight, maxHeight);

  let left = input.pointerX;
  let top = input.pointerY;

  if (left + input.menuWidth > input.viewportWidth - padding) {
    left = input.viewportWidth - input.menuWidth - padding;
  }
  if (left < padding) {
    left = padding;
  }

  const canFitBelow = input.pointerY + renderedHeight <= input.viewportHeight - padding;
  const canFitAbove = input.pointerY - renderedHeight >= padding;
  if (!canFitBelow && canFitAbove) {
    top = input.pointerY - renderedHeight;
  }

  if (top + renderedHeight > input.viewportHeight - padding) {
    top = input.viewportHeight - renderedHeight - padding;
  }
  if (top < padding) {
    top = padding;
  }

  return { left, top, maxHeight };
}

export interface AnchoredDropdownPlacementInput {
  anchorTop: number;
  anchorBottom: number;
  anchorRight: number;
  menuWidth: number;
  preferredHeight: number;
  viewportWidth: number;
  viewportHeight: number;
  viewportPadding?: number;
  menuGap?: number;
  minMenuHeight?: number;
}

export interface AnchoredDropdownPlacement {
  left: number;
  top: number;
  maxHeight: number;
  openUp: boolean;
}

/**
 * Compute a viewport-safe placement for button-anchored dropdown menus.
 */
export function computeAnchoredDropdownPlacement(
  input: AnchoredDropdownPlacementInput,
): AnchoredDropdownPlacement {
  const padding = input.viewportPadding ?? 8;
  const gap = input.menuGap ?? 6;
  const minMenuHeight = input.minMenuHeight ?? 140;

  let left = input.anchorRight - input.menuWidth;
  if (left + input.menuWidth > input.viewportWidth - padding) {
    left = input.viewportWidth - input.menuWidth - padding;
  }
  if (left < padding) {
    left = padding;
  }

  const spaceBelow = input.viewportHeight - input.anchorBottom - padding;
  const spaceAbove = input.anchorTop - padding;
  const openUp = spaceBelow < minMenuHeight && spaceAbove > spaceBelow;

  let maxHeight: number;
  let top: number;
  if (openUp) {
    maxHeight = Math.max(minMenuHeight, spaceAbove - gap);
    const usedHeight = Math.min(input.preferredHeight, maxHeight);
    top = Math.max(padding, input.anchorTop - gap - usedHeight);
  } else {
    maxHeight = Math.max(minMenuHeight, spaceBelow - gap);
    top = input.anchorBottom + gap;
  }

  return { left, top, maxHeight, openUp };
}
