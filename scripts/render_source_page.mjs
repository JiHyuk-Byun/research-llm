#!/usr/bin/env node

import { chromium } from "playwright";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";

const args = process.argv.slice(2);

function takeFlag(name, fallback = undefined) {
  const idx = args.indexOf(name);
  if (idx === -1) return fallback;
  const value = args[idx + 1];
  args.splice(idx, 2);
  return value ?? fallback;
}

function hasFlag(name) {
  const idx = args.indexOf(name);
  if (idx === -1) return false;
  args.splice(idx, 1);
  return true;
}

const outPath = takeFlag("--out");
const waitSelector = takeFlag("--wait-selector");
const timeoutMs = Number(takeFlag("--timeout-ms", "30000"));
const limit = Number(takeFlag("--limit", "200"));
const includeHtml = hasFlag("--include-html");
const url = args[0];

if (!url || url === "--help" || url === "-h") {
  console.error(
    "Usage: node scripts/render_source_page.mjs <url> [--out path] [--wait-selector css] [--timeout-ms ms] [--limit n] [--include-html]",
  );
  process.exit(url ? 0 : 2);
}

function emit(payload) {
  const text = `${JSON.stringify(payload, null, 2)}\n`;
  if (outPath) {
    return mkdir(path.dirname(outPath), { recursive: true }).then(() =>
      writeFile(outPath, text, "utf8"),
    );
  }
  process.stdout.write(text);
  return Promise.resolve();
}

function failure(message, extra = {}) {
  return {
    url,
    status: "failed",
    method: "playwright",
    records_found: 0,
    records: [],
    notes: message,
    ...extra,
  };
}

let browser;
try {
  browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({
    viewport: { width: 1440, height: 1200 },
    userAgent:
      "Mozilla/5.0 research-os source renderer (+https://local.research-os)",
  });

  const response = await page.goto(url, {
    waitUntil: "domcontentloaded",
    timeout: timeoutMs,
  });

  try {
    await page.waitForLoadState("networkidle", { timeout: Math.min(timeoutMs, 15000) });
  } catch {
    // Many conference sites keep long-lived connections open. DOM extraction can still proceed.
  }

  if (waitSelector) {
    await page.waitForSelector(waitSelector, { timeout: timeoutMs });
  }

  const data = await page.evaluate(
    ({ limit, includeHtml }) => {
      const normalize = (value) => (value || "").replace(/\s+/g, " ").trim();
      const absoluteUrl = (href) => {
        try {
          return new URL(href, document.baseURI).href;
        } catch {
          return href || "";
        }
      };
      const isNavigationText = (text) =>
        /^(home|program|schedule|sponsors|login|logout|help|about|search|menu|next|previous)$/i.test(
          text,
        );
      const looksLikeSourceLink = (href, text) => {
        const haystack = `${href} ${text}`.toLowerCase();
        return (
          /paper|poster|oral|abstract|presentation|proceedings|forum|pdf/.test(haystack) &&
          !/sponsor|organizer|registration|attend|policy|privacy/.test(haystack)
        );
      };
      const isSamePageHash = (href) => {
        try {
          const target = new URL(href, document.baseURI);
          return (
            target.origin === location.origin &&
            target.pathname === location.pathname &&
            target.search === location.search &&
            Boolean(target.hash)
          );
        } catch {
          return false;
        }
      };
      const isUtilityLink = (href, text) =>
        /[?&]filter=author\b|[?&]search=|bookmark|calendar|ical|my_schedule|reset|forgot|login/i.test(
          `${href} ${text}`,
        );
      const isBadContainer = (node) =>
        Boolean(
          node?.closest?.(
            "nav, header, footer, aside, .navbar, .nav, .menu, .breadcrumb, .pagination, .osano-cm-window, .osano-cm-dialog, .gdpr, [role='navigation']",
          ),
        );
      const isMainContentLink = (anchor, href, text) =>
        text.length >= 30 &&
        !isSamePageHash(href) &&
        !/^(javascript:|mailto:|tel:)/i.test(href) &&
        Boolean(anchor.closest("main, article, .container, .content, #main"));
      const isPdfLink = (anchor) =>
        /\.pdf($|[\s?#])|\/pdf($|[\s?#])|\bpdf\b/i.test(
          `${anchor.getAttribute("href") || ""} ${normalize(anchor.textContent)}`,
        );
      const nearestRecordRoot = (anchor) =>
        anchor.closest(
          "article, li, tr, .paper, .poster, .oral, .card, .session, .presentation, [class*='paper'], [class*='poster'], [class*='oral'], [class*='card']",
        ) || anchor.parentElement;
      const titleFromRoot = (root, anchor) => {
        const heading = root?.querySelector?.("h1, h2, h3, h4, h5, .title, [class*='title']");
        const headingText = normalize(heading?.textContent);
        if (headingText && !isNavigationText(headingText)) return headingText;
        const anchorText = normalize(anchor.textContent);
        if (anchorText && !isNavigationText(anchorText)) return anchorText;
        return normalize(root?.textContent).slice(0, 180);
      };
      const authorsFromRoot = (root) => {
        const authorLinks = Array.from(root?.querySelectorAll?.("a[href*='filter=author']") || [])
          .map((item) => normalize(item.textContent))
          .filter(Boolean);
        if (authorLinks.length > 0) return authorLinks.slice(0, 30);
        const authorNode = root?.querySelector?.(
          ".authors, .author, [class*='author'], [data-authors]",
        );
        const text = normalize(authorNode?.getAttribute?.("data-authors") || authorNode?.textContent);
        if (!text) return [];
        return text
          .split(/,|;|\band\b/)
          .map((item) => normalize(item))
          .filter(Boolean)
          .slice(0, 30);
      };
      const snippetFromRoot = (root, title) => {
        const abstractNode = root?.querySelector?.(
          ".abstract, [class*='abstract'], .snippet, [class*='snippet'], p",
        );
        const text = normalize(abstractNode?.textContent || root?.textContent);
        if (!text) return "";
        return normalize(text.replace(title, "")).slice(0, 1000);
      };
      const pdfFromRoot = (root) => {
        const pdf = Array.from(root?.querySelectorAll?.("a[href]") || []).find((a) =>
          /\.pdf($|[?#])|\/pdf(\?|$)|pdf/i.test(a.getAttribute("href") || normalize(a.textContent)),
        );
        return pdf ? absoluteUrl(pdf.getAttribute("href")) : undefined;
      };

      const byKey = new Map();
      for (const anchor of Array.from(document.querySelectorAll("a[href]"))) {
        const href = absoluteUrl(anchor.getAttribute("href"));
        const text = normalize(anchor.textContent);
        if (!href || isBadContainer(anchor)) continue;
        if (isUtilityLink(href, text)) continue;
        if (!looksLikeSourceLink(href, text) && !isMainContentLink(anchor, href, text)) continue;

        const root = nearestRecordRoot(anchor);
        const hasNonPdfSourceLink = Array.from(root?.querySelectorAll?.("a[href]") || []).some(
          (candidate) =>
            candidate !== anchor &&
            !isPdfLink(candidate) &&
            looksLikeSourceLink(candidate.getAttribute("href") || "", normalize(candidate.textContent)),
        );
        if (isPdfLink(anchor) && hasNonPdfSourceLink) continue;

        const title = titleFromRoot(root, anchor);
        if (!title || title.length < 8 || isNavigationText(title)) continue;

        const urlOrPath = href;
        const key = `${title.toLowerCase()}|${urlOrPath}`;
        if (byKey.has(key)) continue;

        byKey.set(key, {
          title,
          authors_or_org: authorsFromRoot(root),
          url_or_path: urlOrPath,
          pdf_url: pdfFromRoot(root),
          source_type: "paper",
          abstract_or_snippet: snippetFromRoot(root, title),
          discovery_method: "browser_rendered_dom",
          official_confirmation_url: urlOrPath,
          scope_match_rationale: "Extracted from the rendered DOM of an allowed source page.",
        });

        if (byKey.size >= limit) break;
      }

      return {
        title: document.title,
        body_text_length: normalize(document.body?.textContent).length,
        records: Array.from(byKey.values()),
        html: includeHtml ? document.documentElement.outerHTML : undefined,
      };
    },
    { limit, includeHtml },
  );

  await emit({
    url,
    status: "success",
    method: "playwright",
    http_status: response?.status() ?? null,
    page_title: data.title,
    body_text_length: data.body_text_length,
    records_found: data.records.length,
    records: data.records,
    notes:
      data.records.length > 0
        ? "Rendered DOM extraction completed."
        : "Rendered DOM loaded, but no source-like records were detected by the generic extractor.",
    ...(includeHtml ? { rendered_html: data.html } : {}),
  });
} catch (error) {
  await emit(failure(error?.message || String(error), { error_name: error?.name }));
  process.exitCode = 1;
} finally {
  if (browser) await browser.close();
}
