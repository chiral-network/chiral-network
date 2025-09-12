<script>
  let count = 0
</script>

<div style="padding: 20px; background: #1a1a1a; color: white; min-height: 100vh;">
  <h1>Chiral Network Test</h1>
  <p>If you can see this, the app is working!</p>
  <button on:click={() => count++} style="padding: 10px; background: blue; color: white; border: none; cursor: pointer;">
    Count: {count}
  </button>
</div>