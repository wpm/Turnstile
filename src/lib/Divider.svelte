<script lang="ts">
  let {
    orientation,
    onDragStart,
  }: {
    orientation: "horizontal" | "vertical";
    onDragStart: (e: MouseEvent) => void;
  } = $props();
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="divider {orientation}" onmousedown={onDragStart}>
  <div class="handle">
    <span></span><span></span><span></span>
  </div>
</div>

<style>
  .divider {
    flex-shrink: 0;
    background: var(--color-divider);
    display: flex;
    align-items: center;
    justify-content: center;
    transition: background 0.15s;
  }

  .divider:hover,
  .divider:active {
    background: var(--color-divider-hover);
  }

  .divider.horizontal {
    height: 6px;
    cursor: row-resize;
  }

  .divider.vertical {
    width: 6px;
    cursor: col-resize;
    position: relative;
  }

  .handle {
    display: flex;
    gap: 3px;
    pointer-events: none;
  }

  .horizontal .handle {
    flex-direction: row;
  }

  .vertical .handle {
    flex-direction: column;
  }

  .handle span {
    display: block;
    background: var(--color-handle);
    border-radius: 1px;
  }

  .horizontal .handle span {
    height: 2px;
    width: 6px;
  }

  .vertical .handle span {
    width: 2px;
    height: 6px;
  }

  .divider:hover .handle span,
  .divider:active .handle span {
    background: var(--color-handle-active);
  }
</style>
