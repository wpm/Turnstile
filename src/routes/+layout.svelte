<script lang="ts">
  import "../app.css";
  import { onMount, onDestroy } from "svelte";
  import { startMessageListener } from "$lib/turnstile_messages";
  import type { Snippet } from "svelte";
  const { children }: { children: Snippet } = $props();

  let unlistenMessages: (() => void) | undefined;

  onMount(async () => {
    unlistenMessages = await startMessageListener();
  });

  onDestroy(() => {
    unlistenMessages?.();
  });
</script>

{@render children()}
