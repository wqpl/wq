export function resolveRoute(pathname, search = "") {
  const outerParams = new URLSearchParams(search);
  const routedFile = outerParams.get("route");
  const slug = outerParams.get("slug");
  const section = outerParams.get("section");
  const pathFile = pathname.split("/").pop() || "index.html";
  const file = routedFile || pathFile;
  const params = new URLSearchParams(search);
  if (routedFile) {
    params.delete("route");
  }
  if (!routedFile && slug) {
    return {
      key: `article:${slug}`,
      area: "featured",
      params
    };
  }
  if (!routedFile && section) {
    return {
      key: `subfolder:${section}`,
      area: "featured",
      params
    };
  }
  if (file === "playground.html") {
    return { key: "playground", area: "playground", params };
  }
  if (file === "viz.html") {
    return { key: "viz", area: "viz", params };
  }
  if (file === "repl.html") {
    return { key: "repl", area: "repl", params };
  }
  if (file === "more.html") {
    return { key: "more", area: "more", params };
  }
  if (file === "subfolder.html") {
    return {
      key: `subfolder:${params.get("section") || "Basics"}`,
      area: "featured",
      params
    };
  }
  if (file === "article.html") {
    return {
      key: `article:${params.get("slug") || ""}`,
      area: "featured",
      params
    };
  }
  return { key: "featured", area: "featured", params };
}
