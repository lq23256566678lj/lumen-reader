(() => {
  const welcome = document.getElementById("welcome");
  const reader = document.getElementById("reader");
  const fileInput = document.getElementById("file");
  const importInput = document.getElementById("import-file");
  const drop = document.getElementById("drop");
  const errorBox = document.getElementById("error");
  const titleEl = document.getElementById("book-title");
  const authorEl = document.getElementById("book-author");
  const tocEl = document.getElementById("toc");
  const frame = document.getElementById("page");
  const counter = document.getElementById("counter");
  const prev = document.getElementById("prev");
  const next = document.getElementById("next");
  const back = document.getElementById("back");
  const shelf = document.getElementById("shelf");
  const booksEl = document.getElementById("books");
  const foldersEl = document.getElementById("folders");
  const folderActions = document.getElementById("folder-actions");
  const searchEl = document.getElementById("search");
  const sortEl = document.getElementById("sort");
  const emptyEl = document.getElementById("shelf-empty");
  const importBtn = document.getElementById("import");
  const newFolderBtn = document.getElementById("new-folder");
  const renameFolderBtn = document.getElementById("rename-folder");
  const deleteFolderBtn = document.getElementById("delete-folder");
  const syncBtn = document.getElementById("sync");
  const addBookmarkBtn = document.getElementById("add-bookmark");
  const bookmarksEl = document.getElementById("bookmarks");
  const settingsToggle = document.getElementById("settings-toggle");
  const settingsPanel = document.getElementById("settings-panel");
  const settingsClose = document.getElementById("settings-close");
  const fontSizeEl = document.getElementById("font-size");
  const lineHeightEl = document.getElementById("line-height");
  const themeEl = document.getElementById("theme");
  const fontFamilyEl = document.getElementById("font-family");
  const davUrlEl = document.getElementById("dav-url");
  const davUsernameEl = document.getElementById("dav-username");
  const davPasswordEl = document.getElementById("dav-password");
  const davRemoteDirEl = document.getElementById("dav-remote-dir");
  const davTestBtn = document.getElementById("dav-test");
  const davSyncBtn = document.getElementById("dav-sync");
  const davStatusEl = document.getElementById("dav-status");

  let book = null;
  let current = 0;
  let resetScrollOnLoad = true;
  let library = { books: [], collections: [] };
  let folder = null;
  let query = "";
  let currentBookId = null;
  let currentProgress = null;
  let bookmarks = [];
  let settings = { font_size: 18, line_height: 1.8, theme: "dark", font_family: "" };
  let progressTimer = null;
  let settingsTimer = null;

  const encodePath = (path) =>
    path.split("/").map((part) => encodeURIComponent(part)).join("/");

  function setError(message) {
    errorBox.textContent = message || "";
  }

  function nowSeconds() {
    return Math.floor(Date.now() / 1000);
  }

  function showWelcome() {
    reader.classList.add("hidden");
    welcome.classList.remove("hidden");
    frame.src = "about:blank";
    book = null;
    currentBookId = null;
    currentProgress = null;
    loadLibrary();
  }

  async function loadLibrary() {
    try {
      const response = await fetch("/api/library");
      if (!response.ok) return;
      library = await response.json();
      if (!library || !Array.isArray(library.books)) library = { books: [], collections: [] };
      renderFolders();
      renderBooks();
      shelf.classList.toggle("hidden", library.books.length === 0);
    } catch (_) {
      shelf.classList.add("hidden");
    }
  }

  function renderFolders() {
    foldersEl.innerHTML = "";
    const all = document.createElement("button");
    all.className = "folder-chip" + (folder === null ? " active" : "");
    all.textContent = "全部";
    all.addEventListener("click", () => {
      folder = null;
      renderFolders();
      renderBooks();
    });
    foldersEl.appendChild(all);
    (library.collections || []).filter((c) => !c.deleted).forEach((collection) => {
      const chip = document.createElement("button");
      chip.className = "folder-chip" + (folder === collection.id ? " active" : "");
      chip.textContent = collection.name;
      chip.addEventListener("click", () => {
        folder = collection.id;
        renderFolders();
        renderBooks();
      });
      foldersEl.appendChild(chip);
    });
    folderActions.classList.toggle("hidden", folder === null);
  }

  function parseVolume(title) {
    let text = String(title || "").trim().replace(/[）)\]】》。.!！\s]+$/g, "");
    let base = text;
    let volume = 0;
    const markers = { 上: 1, 前: 1, 中: 2, 下: 3, 后: 3, 续: 4, 终: 5, 完: 5 };
    const last = text.slice(-1);
    if (markers[last]) {
      volume = markers[last];
      base = text.slice(0, -1);
    } else {
      const match = text.match(/^(.*?)[\s·\-—_]*(?:第)?([0-9]+|[零〇一二三四五六七八九十百两]+)(?:[册部卷集])?$/);
      if (match && match[1].trim()) {
        base = match[1];
        const digits = match[2];
        if (/^[0-9]+$/.test(digits)) {
          volume = Number(digits);
        } else {
          let total = 0;
          for (const character of digits) {
            if (character === "十") total = (total || 1) * 10;
            else if (character === "百") total = (total || 1) * 100;
            else {
              const num = "零〇一二三四五六七八九".indexOf(character);
              if (num < 0) {
                volume = 0;
                break;
              }
              total += num;
            }
          }
          volume = total;
        }
      }
    }
    base = base
      .replace(/[（(）)\[\]【】《》·.,，。:：\-_/\\\s]/g, "")
      .toLowerCase();
    return { base: base || text.toLowerCase(), volume };
  }

  function smartCompare(left, right) {
    const a = parseVolume(left.title);
    const b = parseVolume(right.title);
    const byBase = a.base.localeCompare(b.base, "zh-CN", { numeric: true });
    if (byBase !== 0) return byBase;
    if (a.volume !== b.volume) return a.volume - b.volume;
    return String(left.title).localeCompare(String(right.title), "zh-CN", { numeric: true });
  }

  function renderBooks() {
    booksEl.innerHTML = "";
    const lowered = query.trim().toLowerCase();
    const books = (library.books || []).filter((item) => {
      if (folder && item.collection_id !== folder) return false;
      if (!lowered) return true;
      return `${item.title || ""} ${item.author || ""}`.toLowerCase().includes(lowered);
    });
    const mode = sortEl.value;
    books.sort((left, right) => {
      if (mode === "recent") {
        return (right.last_opened_at || 0) - (left.last_opened_at || 0);
      }
      if (mode === "added") return (right.added_at || 0) - (left.added_at || 0);
      if (mode === "smart") return smartCompare(left, right);
      return String(left.title || "").localeCompare(String(right.title || ""), "zh-CN", { numeric: true });
    });
    emptyEl.classList.toggle("hidden", books.length > 0);
    books.forEach((item) => {
      const card = document.createElement("button");
      card.className = "book-card";
      card.title = "点击打开，右键移动到文件夹";
      const cover = document.createElement("img");
      cover.className = "book-cover";
      cover.alt = item.title || "封面";
      if (item.cover_file) {
        cover.src = `/cover/${encodeURIComponent(item.cover_file)}`;
        cover.onerror = () => {
          const fallback = document.createElement("div");
          fallback.className = "book-cover fallback";
          fallback.textContent = item.format || "BOOK";
          cover.replaceWith(fallback);
        };
      } else {
        const fallback = document.createElement("div");
        fallback.className = "book-cover fallback";
        fallback.textContent = (item.format || "BOOK").toUpperCase();
        card.appendChild(fallback);
      }
      if (item.cover_file) card.appendChild(cover);
      const name = document.createElement("span");
      name.className = "book-name";
      name.textContent = item.title || "未命名";
      const author = document.createElement("span");
      author.className = "book-author";
      const progress = item.progress && item.chapter_count
        ? ` · ${Math.round(((item.progress.chapter + (item.progress.fraction || 0)) / item.chapter_count) * 100)}%`
        : "";
      author.textContent = (item.author || "") + progress;
      card.appendChild(name);
      card.appendChild(author);
      card.addEventListener("click", () => openLibraryBook(item.id));
      card.addEventListener("contextmenu", async (event) => {
        event.preventDefault();
        await moveBookPrompt(item);
      });
      booksEl.appendChild(card);
    });
  }

  async function moveBookPrompt(item) {
    const active = (library.collections || []).filter((c) => !c.deleted);
    const names = ["0 = 不放入文件夹", ...active.map((c, index) => `${index + 1} = ${c.name}`)];
    const answer = window.prompt(`把《${item.title}》移动到：\n${names.join("\n")}`, "0");
    if (answer === null) return;
    const index = Number(answer.trim());
    if (Number.isNaN(index)) return;
    const collectionId = index === 0 ? null : active[index - 1] ? active[index - 1].id : null;
    await fetch("/move-book", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ book_id: item.id, collection_id: collectionId }),
    });
    await loadLibrary();
  }

  async function openLibraryBook(id) {
    setError("");
    try {
      const response = await fetch("/open-id", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ id }),
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || "无法打开这本书");
      const record = (library.books || []).find((item) => item.id === id);
      showBook(result, record || null);
    } catch (error) {
      setError(error.message || String(error));
    }
  }

  async function importFile(file) {
    setError("");
    if (!file) return;
    try {
      const response = await fetch("/import", {
        method: "POST",
        headers: { "X-Filename": encodeURIComponent(file.name) },
        body: file,
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || "导入失败");
      await loadLibrary();
      if (result.id) openLibraryBook(result.id);
    } catch (error) {
      setError(error.message || String(error));
    }
  }

  async function openFile(file) {
    setError("");
    if (!file) return;
    if (!/\.epub$/i.test(file.name)) {
      setError("拖入文件只直接打开 EPUB；其他格式请用“导入电子书”加入书架后打开。");
      return;
    }
    drop.classList.add("over");
    drop.querySelector("strong").textContent = "正在解包并分析章节…";
    try {
      const response = await fetch("/open", {
        method: "POST",
        headers: { "Content-Type": "application/epub+zip" },
        body: file,
      });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || "无法打开这本 EPUB");
      showBook(result, null);
    } catch (error) {
      setError(error.message || String(error));
    } finally {
      drop.classList.remove("over");
      drop.querySelector("strong").textContent = "选择一本 EPUB";
    }
  }

  function leadingVolumeImages(doc, pageUrl) {
    const images = [];
    const walk = (nodes) => {
      for (const node of nodes) {
        if (node.nodeType === 3) {
          if (node.textContent.trim()) return false;
          continue;
        }
        if (node.nodeType !== 1) continue;
        const tag = node.tagName;
        const tokens = (node.getAttribute("class") || "").toLowerCase().split(/\s+/);
        if (/^H[1-6]$/.test(tag) || tokens.some((value) => /volumetitle|chaptertitle|titlepage|coverpage/.test(value))) continue;
        if (tag === "BR" || tag === "HR") continue;
        if (tokens.includes("chaptercontent")) {
          if (!walk(node.childNodes)) return false;
          continue;
        }
        if (tokens.some((value) => /divimage|illus|duokan-image-single|color-page/.test(value))) {
          node.querySelectorAll("img").forEach((image) => {
            const src = image.getAttribute("src");
            if (src) images.push(new URL(src, pageUrl).href);
          });
          continue;
        }
        if (node.textContent.trim()) return false;
      }
      return true;
    };
    walk(doc.body ? doc.body.childNodes : []);
    return [...new Set(images)];
  }

  async function consolidateColorPages(result) {
    if (!result || result.kind !== "epub" || !result.spine) return;
    if (result.spine.some((item) => item.title && item.title.startsWith("彩页"))) return;
    const volumes = [];
    for (let index = 0; index < result.spine.length; index += 1) {
      const item = result.spine[index];
      if (!/\.x?html?$/i.test(item.href)) continue;
      try {
        const pageUrl = new URL(`/book/${encodePath(item.href)}`, location.origin);
        const response = await fetch(pageUrl);
        if (!response.ok) continue;
        const doc = new DOMParser().parseFromString(await response.text(), "text/html");
        const images = leadingVolumeImages(doc, pageUrl);
        if (images.length) volumes.push({ index, images, title: item.title || `第 ${index + 1} 卷` });
      } catch (_) {
        // Not an XHTML page.
      }
    }
    if (!volumes.length) return;

    const hidden = new Set();
    volumes.forEach((volume) => volume.images.forEach((src) => hidden.add(src)));
    const map = new Map();
    const spine = [];
    for (let index = 0; index < result.spine.length; index += 1) {
      const volume = volumes.find((item) => item.index === index);
      if (volume) {
        const html = `<meta charset="utf-8"><style>html,body{margin:0;padding:0;background:#111}body{display:flex;flex-direction:column;align-items:center}.color-page{display:block;width:100%;max-width:100%;height:auto;margin:0;padding:0;border:0}</style>${volume.images.map((src) => `<img class="color-page" src="${src}">`).join("")}`;
        volume.blob = URL.createObjectURL(new Blob([html], { type: "text/html;charset=utf-8" }));
        spine.push({ href: volume.blob, title: `彩页 · ${volume.title}` });
      }
      map.set(index, spine.length);
      spine.push(result.spine[index]);
    }
    result.spine = spine;
    result.hiddenImages = [...hidden];

    if (result.toc && result.toc.length) {
      const toc = [];
      const inserted = new Set();
      result.toc.forEach((entry) => {
        volumes.forEach((volume) => {
          if (!inserted.has(volume.index) && entry.spine >= volume.index) {
            toc.push({ title: `彩页 · ${volume.title}`, href: volume.blob, fragment: "", spine: map.get(volume.index) - 1 });
            inserted.add(volume.index);
          }
        });
        toc.push({ ...entry, spine: map.get(entry.spine) ?? Math.min(entry.spine, result.spine.length - 1) });
      });
      volumes.forEach((volume) => {
        if (!inserted.has(volume.index)) {
          toc.push({ title: `彩页 · ${volume.title}`, href: volume.blob, fragment: "", spine: map.get(volume.index) - 1 });
        }
      });
      result.toc = toc;
    }

    if (currentProgress) {
      const oldChapter = currentProgress.chapter || 0;
      currentProgress.chapter = map.get(oldChapter) ?? Math.min(oldChapter, result.spine.length - 1);
    }
  }

  async function showBook(result, record) {
    currentBookId = record ? record.id : null;
    currentProgress = record && record.progress ? record.progress : null;
    if (currentProgress && Array.isArray(result.spine_map) && result.spine_map.length) {
      const oldChapter = currentProgress.chapter || 0;
      currentProgress.chapter = result.spine_map[oldChapter] ?? Math.min(oldChapter, result.spine.length - 1);
    }
    await consolidateColorPages(result);
    book = result;
    current = 0;
    titleEl.textContent = book.title || "未命名";
    authorEl.textContent = book.author || "";
    welcome.classList.add("hidden");
    reader.classList.remove("hidden");
    reader.classList.toggle("pdf-mode", book.kind !== "epub");
    if (book.kind !== "epub") {
      tocEl.innerHTML = "";
      frame.src = "/bookfile";
      counter.textContent = (book.kind || "FILE").toUpperCase();
      prev.disabled = true;
      next.disabled = true;
      loadBookmarks();
      return;
    }
    renderToc();
    const chapter = currentProgress ? Math.min(currentProgress.chapter || 0, book.spine.length - 1) : 0;
    showSpine(chapter, currentProgress ? currentProgress.fraction : 0);
    loadBookmarks();
  }

  async function bootstrap() {
    await loadSettings();
    try {
      const response = await fetch("/api/book");
      if (response.ok) {
        const result = await response.json();
        if (result && (result.kind !== "epub" || (result.spine && result.spine.length))) {
          showBook(result, null);
          return;
        }
      }
    } catch (_) {
      // fall through to shelf
    }
    await loadLibrary();
  }

  function hrefFor(item, withFragment = true) {
    if (/^(blob:|data:|https?:)/i.test(item.href)) return item.href;
    const path = "/book/" + encodePath(item.href);
    return withFragment && item.fragment ? `${path}#${item.fragment}` : path;
  }

  function renderToc() {
    tocEl.innerHTML = "";
    (book.toc || []).forEach((item) => {
      const button = document.createElement("button");
      button.textContent = item.title || `第 ${item.spine + 1} 节`;
      button.dataset.spine = String(item.spine);
      button.addEventListener("click", () => {
        resetScrollOnLoad = !item.fragment;
        frame.dataset.pendingFraction = "0";
        frame.src = hrefFor(item);
        setActive(item.spine);
        current = item.spine;
        updateCounter();
        scheduleProgress();
      });
      tocEl.appendChild(button);
    });
    setActive(0);
  }

  function setActive(spine) {
    tocEl.querySelectorAll("button").forEach((button) => {
      button.classList.toggle("active", Number(button.dataset.spine) === spine);
    });
  }

  function showSpine(index, fraction = 0) {
    if (!book || !book.spine || !book.spine.length) return;
    current = Math.max(0, Math.min(index, book.spine.length - 1));
    const item = book.spine[current];
    resetScrollOnLoad = fraction <= 0;
    frame.dataset.pendingFraction = String(fraction || 0);
    frame.src = hrefFor(item);
    setActive(current);
    updateCounter();
  }

  function frameScrollFraction() {
    try {
      const scroll = frame.contentWindow;
      if (!scroll) return 0;
      const doc = scroll.document;
      const max = Math.max(1, doc.documentElement.scrollHeight - scroll.innerHeight);
      return Math.min(1, Math.max(0, scroll.scrollY / max));
    } catch (_) {
      return 0;
    }
  }

  function scheduleProgress() {
    if (!currentBookId || !book || book.kind !== "epub") return;
    if (progressTimer) clearTimeout(progressTimer);
    progressTimer = setTimeout(saveProgress, 600);
  }

  async function saveProgress() {
    if (!currentBookId || !book || book.kind !== "epub") return;
    try {
      await fetch("/progress", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          book_id: currentBookId,
          chapter: current,
          fraction: frameScrollFraction(),
          scroll_offset: 0,
        }),
      });
    } catch (_) {
      // 保存进度失败不打断阅读。
    }
  }

  function applyFrameSettings() {
    if (!book || book.kind === "pdf") return;
    try {
      const doc = frame.contentDocument;
      if (!doc || !doc.head) return;
      let style = doc.getElementById("lumen-reader-settings");
      if (!style) {
        style = doc.createElement("style");
        style.id = "lumen-reader-settings";
        doc.head.appendChild(style);
      }
      style.textContent = `
        html, body {
          font-size: ${settings.font_size}px !important;
          line-height: ${settings.line_height} !important;
          ${settings.font_family ? `font-family: ${settings.font_family} !important;` : ""}
        }
      `;
    } catch (_) {
      // cross-origin / PDF
    }
  }

  function hideConsolidatedImages() {
    if (!book || !Array.isArray(book.hiddenImages) || !book.hiddenImages.length) return;
    try {
      const hidden = new Set(book.hiddenImages);
      frame.contentDocument.querySelectorAll("img").forEach((image) => {
        if (hidden.has(image.src)) image.style.display = "none";
      });
    } catch (_) {
      // ignore
    }
  }

  function resumeScroll() {
    const fraction = Number(frame.dataset.pendingFraction || 0);
    try {
      const win = frame.contentWindow;
      if (!win) return;
      if (fraction > 0) {
        const max = Math.max(0, win.document.documentElement.scrollHeight - win.innerHeight);
        win.scrollTo(0, max * fraction);
      } else if (resetScrollOnLoad) {
        win.scrollTo(0, 0);
      }
    } catch (_) {
      // ignore
    }
    frame.dataset.pendingFraction = "0";
    applyFrameSettings();
    hideConsolidatedImages();
    try {
      frame.contentWindow.addEventListener("scroll", scheduleProgress, { passive: true });
    } catch (_) {
      // ignore
    }
  }

  frame.addEventListener("load", resumeScroll);

  function updateCounter() {
    if (!book || book.kind !== "epub") return;
    counter.textContent = `${current + 1} / ${book.spine.length}`;
    prev.disabled = current <= 0;
    next.disabled = current >= book.spine.length - 1;
  }

  async function loadBookmarks() {
    bookmarksEl.innerHTML = "";
    if (!currentBookId) return;
    try {
      const response = await fetch(`/api/bookmarks?book_id=${encodeURIComponent(currentBookId)}`);
      if (!response.ok) return;
      bookmarks = await response.json();
    } catch (_) {
      bookmarks = [];
    }
    if (!bookmarks.length) {
      bookmarksEl.innerHTML = '<p class="shelf-empty">还没有书签</p>';
      return;
    }
    bookmarks.sort((a, b) => a.chapter - b.chapter || (a.fraction || 0) - (b.fraction || 0));
    bookmarks.forEach((item) => {
      const row = document.createElement("div");
      row.className = "bookmark";
      const button = document.createElement("button");
      button.className = "bookmark";
      button.textContent = item.label || `第 ${item.chapter + 1} 章`;
      button.addEventListener("click", () => {
        showSpine(item.chapter, item.fraction || 0);
      });
      const remove = document.createElement("button");
      remove.textContent = "×";
      remove.title = "删除书签";
      remove.addEventListener("click", async (event) => {
        event.stopPropagation();
        await fetch(`/api/bookmarks/${encodeURIComponent(item.id)}`, { method: "DELETE" });
        loadBookmarks();
      });
      row.appendChild(button);
      row.appendChild(remove);
      bookmarksEl.appendChild(row);
    });
  }

  async function addBookmark() {
    if (!currentBookId || !book) {
      setError("直接打开的文件不会保存书签；请先导入书架。");
      return;
    }
    const label = window.prompt("书签名称", `第 ${current + 1} 章`);
    if (label === null) return;
    await fetch("/api/bookmarks", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        book_id: currentBookId,
        chapter: current,
        fraction: frameScrollFraction(),
        label: label.trim() || `第 ${current + 1} 章`,
      }),
    });
    loadBookmarks();
  }

  async function loadSettings() {
    try {
      const response = await fetch("/api/settings");
      if (response.ok) settings = { ...settings, ...(await response.json()) };
    } catch (_) {
      // defaults
    }
    fontSizeEl.value = settings.font_size;
    lineHeightEl.value = settings.line_height;
    themeEl.value = settings.theme;
    fontFamilyEl.value = settings.font_family || "";
    document.body.dataset.theme = settings.theme || "dark";
  }

  function scheduleSaveSettings() {
    document.body.dataset.theme = themeEl.value;
    settings = {
      font_size: Number(fontSizeEl.value),
      line_height: Number(lineHeightEl.value),
      theme: themeEl.value,
      font_family: fontFamilyEl.value,
    };
    applyFrameSettings();
    if (settingsTimer) clearTimeout(settingsTimer);
    settingsTimer = setTimeout(() => {
      fetch("/api/settings", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(settings),
      }).catch(() => {});
    }, 400);
  }

  async function loadWebdav() {
    try {
      const response = await fetch("/api/webdav");
      if (!response.ok) return;
      const config = await response.json();
      davUrlEl.value = config.url || "";
      davUsernameEl.value = config.username || "";
      davPasswordEl.value = config.password || "";
      davRemoteDirEl.value = config.remote_dir || "";
      davStatusEl.textContent = config.last_sync_status || "";
    } catch (_) {
      // ignore
    }
  }

  function webdavPayload() {
    return {
      url: davUrlEl.value.trim(),
      username: davUsernameEl.value.trim(),
      password: davPasswordEl.value,
      remote_dir: davRemoteDirEl.value.trim(),
    };
  }

  async function saveWebdav() {
    await fetch("/api/webdav", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(webdavPayload()),
    });
  }

  async function testWebdav() {
    davStatusEl.textContent = "正在测试…";
    await saveWebdav();
    try {
      const response = await fetch("/api/webdav/test", { method: "POST" });
      const result = await response.json();
      davStatusEl.textContent = result.message || result.error || "";
    } catch (error) {
      davStatusEl.textContent = String(error);
    }
  }

  async function syncWebdav() {
    davStatusEl.textContent = "正在同步…";
    await saveWebdav();
    try {
      const response = await fetch("/api/sync", { method: "POST" });
      const result = await response.json();
      if (!response.ok) throw new Error(result.error || "同步失败");
      davStatusEl.textContent = result.message || "同步完成";
      await loadLibrary();
      if (currentBookId) {
        const record = (library.books || []).find((item) => item.id === currentBookId);
        if (record && record.progress && book && book.kind === "epub") {
          currentProgress = record.progress;
        }
      }
    } catch (error) {
      davStatusEl.textContent = error.message || String(error);
    }
  }

  fileInput.addEventListener("change", () => openFile(fileInput.files[0]));
  importBtn.addEventListener("click", () => importInput.click());
  importInput.addEventListener("change", () => importFile(importInput.files[0]));
  searchEl.addEventListener("input", () => {
    query = searchEl.value;
    renderBooks();
  });
  sortEl.addEventListener("change", renderBooks);
  newFolderBtn.addEventListener("click", async () => {
    const name = window.prompt("新文件夹名称");
    if (!name || !name.trim()) return;
    await fetch("/folders", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: name.trim() }),
    });
    await loadLibrary();
  });
  renameFolderBtn.addEventListener("click", async () => {
    const active = (library.collections || []).find((item) => item.id === folder);
    if (!active) return;
    const name = window.prompt("重命名文件夹", active.name);
    if (!name || !name.trim()) return;
    await fetch(`/folders/${encodeURIComponent(active.id)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: name.trim() }),
    });
    await loadLibrary();
  });
  deleteFolderBtn.addEventListener("click", async () => {
    const active = (library.collections || []).find((item) => item.id === folder);
    if (!active) return;
    if (!window.confirm(`删除文件夹“${active.name}”？书不会删除。`)) return;
    await fetch(`/folders/${encodeURIComponent(active.id)}`, { method: "DELETE" });
    folder = null;
    await loadLibrary();
  });
  syncBtn.addEventListener("click", async () => {
    settingsPanel.classList.remove("hidden");
    await loadWebdav();
    davStatusEl.textContent = "点击“立即同步”开始。";
  });
  settingsToggle.addEventListener("click", async () => {
    settingsPanel.classList.remove("hidden");
    await loadWebdav();
  });
  settingsClose.addEventListener("click", () => settingsPanel.classList.add("hidden"));
  settingsPanel.addEventListener("click", (event) => {
    if (event.target === settingsPanel) settingsPanel.classList.add("hidden");
  });
  [fontSizeEl, lineHeightEl, themeEl, fontFamilyEl].forEach((element) => {
    element.addEventListener("input", scheduleSaveSettings);
    element.addEventListener("change", scheduleSaveSettings);
  });
  davTestBtn.addEventListener("click", testWebdav);
  davSyncBtn.addEventListener("click", syncWebdav);
  addBookmarkBtn.addEventListener("click", addBookmark);
  ["dragenter", "dragover"].forEach((name) => {
    drop.addEventListener(name, (event) => {
      event.preventDefault();
      drop.classList.add("over");
    });
  });
  ["dragleave", "drop"].forEach((name) => {
    drop.addEventListener(name, (event) => {
      event.preventDefault();
      drop.classList.remove("over");
    });
  });
  drop.addEventListener("drop", (event) => openFile(event.dataTransfer.files[0]));
  prev.addEventListener("click", () => showSpine(current - 1));
  next.addEventListener("click", () => showSpine(current + 1));
  back.addEventListener("click", async () => {
    await saveProgress();
    showWelcome();
  });
  window.addEventListener("beforeunload", () => {
    if (currentBookId && book && book.kind === "epub") {
      navigator.sendBeacon?.(
        "/progress",
        new Blob([JSON.stringify({
          book_id: currentBookId,
          chapter: current,
          fraction: frameScrollFraction(),
          scroll_offset: 0,
        })], { type: "application/json" })
      );
    }
  });
  window.addEventListener("keydown", (event) => {
    if (event.key === "Escape") settingsPanel.classList.add("hidden");
    if (!book || book.kind !== "epub") return;
    if (event.key === "ArrowRight" || event.key === "PageDown") showSpine(current + 1);
    if (event.key === "ArrowLeft" || event.key === "PageUp") showSpine(current - 1);
  });

  bootstrap();
})();
