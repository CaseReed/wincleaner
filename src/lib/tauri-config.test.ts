import { describe, it, expect } from "vitest";
import tauriConf from "../../src-tauri/tauri.conf.json";
import defaultCapability from "../../src-tauri/capabilities/default.json";
import sonnerSource from "../components/ui/sonner.tsx?raw";

/// The content security policy is not code: nobody reads it while reviewing a
/// component diff. This test fails if anyone loosens it.
const conf = tauriConf as { app: { security: { csp: string; devCsp?: string } } };
const capabilities = defaultCapability as { permissions: string[] };

describe("CSP", () => {
  const csp = conf.app.security.csp;

  it("forbids the network and remote resources", () => {
    expect(csp).toContain("default-src 'self'");
    expect(csp).toContain("connect-src 'self' ipc: http://ipc.localhost");
    expect(csp).not.toMatch(/https?:\/\/(?!ipc\.localhost)/);
  });

  it("never allows an inline script", () => {
    // `script-src` falls back to `default-src 'self'`: what has to be checked
    // is that nobody added a permissive `script-src`.
    expect(csp).not.toMatch(/script-src[^;]*unsafe-(inline|eval)/);
    expect(csp).not.toContain("'unsafe-eval'");
  });

  it("carries the hardening directives", () => {
    for (const directive of [
      "object-src 'none'",
      "base-uri 'none'",
      "frame-ancestors 'none'",
      "form-action 'none'",
    ]) {
      expect(csp).toContain(directive);
    }
  });

  it("tolerates 'unsafe-inline' only on style attributes", () => {
    // sonner and the @base-ui primitives set inline `style=` attributes:
    // `style-src-attr` covers exactly that case without allowing an injected
    // <style> tag. Stylesheets all go through the Vite bundle instead: see the
    // `sonner/dist/styles.css` import in `src/components/ui/sonner.tsx`,
    // without which sonner would inject its own into a <style> tag that
    // `style-src 'self'` blocks.
    expect(csp).toContain("style-src 'self'");
    expect(csp).not.toContain("style-src 'self' 'unsafe-inline'");
    expect(csp).toContain("style-src-attr 'unsafe-inline'");
  });

  it("lets the sonner stylesheet in through the bundle", () => {
    // The component must import the stylesheet: otherwise the CSP blocks it
    // and toasts render unstyled in release, without anything failing at build
    // time. Vitest neutralises stylesheets: what is actually bundled is checked
    // on the bundle itself
    // (`npm run build` then `grep data-sonner-toaster dist/assets/*.css`).
    expect(sonnerSource).toContain('import "sonner/dist/styles.css"');
  });

  it("keeps a separate development CSP for Vite HMR", () => {
    // Vite injects <style> tags in development: without this, the screen is
    // unstyled under `npm run tauri dev`.
    expect(conf.app.security.devCsp).toContain("style-src 'self' 'unsafe-inline'");
  });
});

describe("capabilities", () => {
  it("grants only what the front end uses", () => {
    // `core:default` expands into path, event, window, webview, app, image,
    // resources, menu and tray — including arbitrary path resolution and
    // `allow-internal-toggle-devtools`. The front end calls one thing only:
    // `getCurrentWindow().setTheme()` (src/App.tsx).
    expect(capabilities.permissions).not.toContain("core:default");
    expect(capabilities.permissions).toContain("core:window:allow-set-theme");
  });

  it("declares no plugin granting disk or network access", () => {
    for (const permission of capabilities.permissions) {
      expect(permission.startsWith("core:")).toBe(true);
    }
  });
});
