import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/// La politique de sécurité du contenu n'est pas du code : personne ne la lit
/// en relisant un diff de composant. Ce test échoue si quelqu'un la relâche.
function lire<T>(chemin: string): T {
  return JSON.parse(readFileSync(resolve(__dirname, "..", "..", chemin), "utf8")) as T;
}

const conf = lire<{ app: { security: { csp: string; devCsp?: string } } }>(
  "src-tauri/tauri.conf.json"
);
const capacites = lire<{ permissions: string[] }>(
  "src-tauri/capabilities/default.json"
);

describe("CSP", () => {
  const csp = conf.app.security.csp;

  it("interdit le réseau et les ressources distantes", () => {
    expect(csp).toContain("default-src 'self'");
    expect(csp).toContain("connect-src 'self' ipc: http://ipc.localhost");
    expect(csp).not.toMatch(/https?:\/\/(?!ipc\.localhost)/);
  });

  it("n'autorise jamais un script en ligne", () => {
    // `script-src` retombe sur `default-src 'self'` : ce qu'il faut vérifier,
    // c'est que personne n'a ajouté un `script-src` permissif.
    expect(csp).not.toMatch(/script-src[^;]*unsafe-(inline|eval)/);
    expect(csp).not.toContain("'unsafe-eval'");
  });

  it("porte les directives de durcissement", () => {
    for (const directive of [
      "object-src 'none'",
      "base-uri 'none'",
      "frame-ancestors 'none'",
      "form-action 'none'",
    ]) {
      expect(csp).toContain(directive);
    }
  });

  it("ne tolère 'unsafe-inline' que sur les attributs de style", () => {
    // sonner et les primitives @base-ui posent des attributs `style=` en
    // ligne : `style-src-attr` couvre exactement ce cas, sans autoriser une
    // balise <style> injectée.
    expect(csp).toContain("style-src 'self'");
    expect(csp).not.toContain("style-src 'self' 'unsafe-inline'");
    expect(csp).toContain("style-src-attr 'unsafe-inline'");
  });

  it("garde une CSP de développement distincte pour le HMR de Vite", () => {
    // Vite injecte des balises <style> en développement : sans cela, l'écran
    // est nu en `npm run tauri dev`.
    expect(conf.app.security.devCsp).toContain("style-src 'self' 'unsafe-inline'");
  });
});

describe("capacités", () => {
  it("n'accorde que ce dont le front se sert", () => {
    // `core:default` se développe en path, event, window, webview, app, image,
    // resources, menu et tray — dont la résolution de chemins arbitraires et
    // `allow-internal-toggle-devtools`. Le front n'appelle qu'une chose :
    // `getCurrentWindow().setTheme()` (src/App.tsx).
    expect(capacites.permissions).not.toContain("core:default");
    expect(capacites.permissions).toContain("core:window:allow-set-theme");
  });

  it("ne déclare aucun plugin donnant accès au disque ou au réseau", () => {
    for (const permission of capacites.permissions) {
      expect(permission.startsWith("core:")).toBe(true);
    }
  });
});
