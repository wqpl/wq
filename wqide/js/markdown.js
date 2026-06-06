// headings, paragraphs, lists, code fences, inline code, bold (**text** -> <strong>), italic (*text* -> <em>), links, escaping

function escapeHtml(str) {
  return str
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/\"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function inlineParse(raw) {
  // Protect inline code first using placeholders
  const codeSpans = [];
  let text = raw.replace(/(`+)([\s\S]*?)(?<!`)\1(?!`)/g, (m, fence, code) => {
    if (!code.trim()) return m;
    const normalized = code.startsWith(" ") && code.endsWith(" ")
      ? code.slice(1, -1)
      : code;
    const idx = codeSpans.push(normalized) - 1;
    return `\uE000CODE${idx}\uE000`;
  });

  // Links: [text](url)
  text = text.replace(
    /\[([^\]]+)\]\(([^)\s]+)(?:\s+\"([^\"]+)\")?\)/g,
    (m, label, url, title) => {
      const t = title ? ` title="${escapeHtml(title)}"` : "";
      return `<a href="${escapeHtml(url)}"${t}>${escapeHtml(label)}</a>`;
    },
  );

  // Bold (**text**)
  text = text.replace(
    /\*\*([^*]+)\*\*/g,
    (m, b) => `<strong>${escapeHtml(b)}</strong>`,
  );

  // Italic (*text*) — avoid matching inside ** ** by using a tempered pattern
  text = text.replace(
    /(^|[^*])\*([^*]+)\*(?!\*)/g,
    (m, pre, i) => `${pre}<em>${escapeHtml(i)}</em>`,
  );

  // Italic (_text_) — avoid matching inside __ __ by using a tempered pattern
  text = text.replace(
    /(^|[^_])_([^_]+)_(?!_)/g,
    (m, pre, i) => `${pre}<em>${escapeHtml(i)}</em>`,
  );

  // Restore code spans (escaped, not further formatted)
  text = text.replace(
    /\uE000CODE(\d+)\uE000/g,
    (m, i) => `<code>${escapeHtml(codeSpans[Number(i)])}</code>`,
  );
  return text;
}

export function parseMarkdown(md) {
  const lines = md.replace(/\r\n?/g, "\n").split("\n");
  const out = [];
  let i = 0;
  let inCode = false;
  let codeFence = "";
  let codeLang = "";
  let codeBuf = [];
  let listStack = [];
  let paraBuf = [];

  // --- Table helpers (GFM-style pipe tables) ---
  function splitTableRow(row) {
    // Split on unescaped pipes. Support \\ and \| escapes within cells.
    let s = row.trim();
    if (s.startsWith("|")) s = s.slice(1);
    if (s.endsWith("|")) s = s.slice(0, -1);
    const cells = [];
    let buf = "";
    for (let i = 0; i < s.length; i++) {
      const ch = s[i];
      if (ch === "\\") {
        if (i + 1 < s.length) {
          // consume next char literally
          buf += s[i + 1];
          i++;
        } else {
          // trailing backslash
          buf += "\\";
        }
      } else if (ch === "|") {
        cells.push(buf.trim());
        buf = "";
      } else {
        buf += ch;
      }
    }
    cells.push(buf.trim());
    return cells;
  }
  function isAlignRow(line) {
    if (!line || !/\|/.test(line)) return false;
    const cells = splitTableRow(line);
    if (!cells.length) return false;
    return cells.every((seg) => /:?\s*-{3,}\s*:?(\s*)?$/.test(seg));
  }
  function parseAlign(seg) {
    const t = seg.replace(/\s+/g, "");
    if (t.startsWith(":") && t.endsWith(":")) return "center";
    if (t.startsWith(":")) return "left";
    if (t.endsWith(":")) return "right";
    return "";
  }
  function isTableStartAt(idx) {
    const headerLine = lines[idx];
    const next = lines[idx + 1];
    if (!headerLine || !next) return false;
    // Must have at least one unescaped pipe to split into >1 cells
    const headerCells = splitTableRow(headerLine);
    if (headerCells.length <= 1) return false;
    return isAlignRow(next);
  }
  function tryParseTable() {
    const headerLine = lines[i];
    const next = lines[i + 1];
    if (!isTableStartAt(i)) return false;
    // Parse header + alignment
    const headers = splitTableRow(headerLine);
    const aligns = splitTableRow(next).map(parseAlign);
    const cols = Math.max(headers.length, aligns.length);
    const al = new Array(cols).fill("").map((_, idx) => aligns[idx] || "");
    i += 2; // consume header + align
    const bodyRows = [];
    while (i < lines.length && /\|/.test(lines[i]) && !/^\s*$/.test(lines[i])) {
      bodyRows.push(splitTableRow(lines[i]));
      i++;
    }
    // Build HTML
    const theadCells = headers
      .map((h, idx) => {
        const a = al[idx] ? ` style="text-align:${al[idx]}"` : "";
        return `<th${a}>${inlineParse(h)}</th>`;
      })
      .join("");
    const thead = `<thead><tr>${theadCells}</tr></thead>`;
    const tbody = `<tbody>${bodyRows
      .map((row) => {
        const cells = row
          .map((c, idx) => {
            const a = al[idx] ? ` style=\"text-align:${al[idx]}\"` : "";
            return `<td${a}>${inlineParse(c)}</td>`;
          })
          .join("");
        return `<tr>${cells}</tr>`;
      })
      .join("")}</tbody>`;
    out.push(`<table>${thead}${tbody}</table>`);
    return true;
  }

  function flushParagraph() {
    if (paraBuf.length) {
      const text = paraBuf.join(" ").trim();
      if (text) out.push(`<p>${inlineParse(text)}</p>`);
      paraBuf = [];
    }
  }
  function renderList(list) {
    const itemsHtml = list.items
      .map((item) => {
        const childrenHtml = item.children.map(renderList).join("");
        return `<li>${inlineParse(item.text)}${childrenHtml}</li>`;
      })
      .join("");
    return `<${list.type}>${itemsHtml}</${list.type}>`;
  }
  function flushList() {
    if (listStack.length) {
      out.push(renderList(listStack[0]));
      listStack = [];
    }
  }
  function getListIndent(rawLine) {
    const match = rawLine.match(/^(\s*)/);
    return match ? match[1].replace(/\t/g, "    ").length : 0;
  }
  function addListItem(type, indent, text) {
    while (listStack.length && indent < listStack[listStack.length - 1].indent) {
      listStack.pop();
    }

    if (!listStack.length) {
      listStack.push({ type, indent, items: [] });
    } else {
      const current = listStack[listStack.length - 1];

      if (indent > current.indent) {
        const parentItem = current.items[current.items.length - 1];
        if (!parentItem) {
          current.items.push({ text, children: [] });
          return;
        }
        const childList = { type, indent, items: [] };
        parentItem.children.push(childList);
        listStack.push(childList);
      } else if (current.type !== type) {
        if (listStack.length === 1) {
          flushList();
          listStack.push({ type, indent, items: [] });
        } else {
          listStack.pop();
          addListItem(type, indent, text);
          return;
        }
      }
    }

    const active = listStack[listStack.length - 1];
    active.items.push({ text, children: [] });
  }
  function flushCode() {
    if (inCode) {
      out.push(
        `<pre><code${codeLang ? ` class="language-${codeLang}"` : ""}>${escapeHtml(codeBuf.join("\n"))}</code></pre>`,
      );
      inCode = false;
      codeFence = "";
      codeLang = "";
      codeBuf = [];
    }
  }

  while (i < lines.length) {
    const line = lines[i];

    // Code fence
    const fenceMatch = line.match(/^(`{3,})\s*([a-zA-Z0-9_-]+)?\s*$/);
    if (fenceMatch) {
      if (!inCode) {
        // entering code
        flushParagraph();
        flushList();
        inCode = true;
        codeFence = fenceMatch[1];
        codeLang = fenceMatch[2] || "";
      } else {
        // leaving code
        if (line.trim() === codeFence) {
          flushCode();
        } else {
          codeBuf.push(line);
        }
      }
      i++;
      continue;
    }

    if (inCode) {
      codeBuf.push(line);
      i++;
      continue;
    }

    // Blank line
    if (/^\s*$/.test(line)) {
      flushParagraph();
      flushList();
      i++;
      continue;
    }

    // Heading #..######
    const h = line.match(/^(#{1,6})\s+(.+)$/);
    if (h) {
      flushParagraph();
      flushList();
      const level = h[1].length;
      const text = h[2].trim();
      out.push(`<h${level}>${inlineParse(text)}</h${level}>`);
      i++;
      continue;
    }

    // Table
    if (isTableStartAt(i)) {
      flushParagraph();
      flushList();
      tryParseTable();
      continue;
    }

    // Ordered list
    const ol = line.match(/^(\s*)\d+\.\s+(.+)$/);
    if (ol) {
      flushParagraph();
      addListItem("ol", getListIndent(line), ol[2]);
      i++;
      continue;
    }
    // Unordered list
    const ul = line.match(/^(\s*)[-*]\s+(.+)$/);
    if (ul) {
      flushParagraph();
      addListItem("ul", getListIndent(line), ul[2]);
      i++;
      continue;
    }

    // Paragraph line (merge consecutive lines)
    paraBuf.push(line.trim());
    i++;
  }
  // final flush
  flushCode();
  flushParagraph();
  flushList();

  return out.join("\n");
}
