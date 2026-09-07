/** 模型目录里只保留页面展示需要的核心字段 */
export function summarizeModel(model) {
  if (!model || typeof model !== "object") return null;
  return {
    slug: model.slug || model.display_name || "",
    display_name: model.display_name || model.slug || "",
    default_reasoning_level: model.default_reasoning_level || "",
    supported_reasoning_levels: (model.supported_reasoning_levels || []).map(
      (level) => ({
        effort: typeof level === "string" ? level : level?.effort || "",
      }),
    ),
    supported_in_api: Boolean(model.supported_in_api),
    visibility: model.visibility || "list",
  };
}

export function summarizeModels(models) {
  return (models || []).map(summarizeModel).filter(Boolean);
}
