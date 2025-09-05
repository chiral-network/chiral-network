<script>
  let count = 0
</script>

<div style="background: black; color: white; padding: 20px; min-height: 100vh;">
  <h1>Simple Test</h1>
  <p>If you see this, Svelte is working!</p>
  <button on:click={() => count++} style="padding: 10px; background: blue; color: white;">
    Clicked {count} times
  </button>
</div>