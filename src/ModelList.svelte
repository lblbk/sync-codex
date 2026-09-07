<script>
  let { models = [] } = $props();
</script>

<div class="model-list">
  {#each models as model}
    <div class="model-card">
      <div class="model-card-top">
        <span class="model-name">{model.display_name || model.slug || "未命名模型"}</span>
        {#if model.default_reasoning_level}
          <span class="tag default">默认: {model.default_reasoning_level}</span>
        {/if}
      </div>
      {#if model.supported_reasoning_levels?.length}
        <div class="levels">
          <span class="levels-label">支持思考等级</span>
          {#each model.supported_reasoning_levels as level}
            <span
              class="tag {level.effort === model.default_reasoning_level ? "active" : ""}"
            >
              {level.effort}
            </span>
          {/each}
        </div>
      {:else}
        <p class="no-levels">未声明支持的思考等级</p>
      {/if}
    </div>
  {/each}
</div>