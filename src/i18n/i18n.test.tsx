import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("@/lib/api", async () => {
  const actual = await vi.importActual<typeof import("@/lib/api")>("@/lib/api");
  return {
    ...actual,
    listRules: () => Promise.resolve([]),
    rulesSummary: () => Promise.resolve(null),
    runningBrowsers: () => Promise.resolve([]),
    sandboxOrphans: () => Promise.resolve([]),
  };
});

vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}));

import { CleanPanel } from "@/components/CleanPanel";
import { SettingsPanel } from "@/components/SettingsPanel";
import { en } from "./en";
import { fr } from "./fr";
import { I18nProvider, LANGUAGE_KEY, readLanguagePreference, resolveLanguage } from ".";

/// `navigator.language` is read at first render: jsdom answers "en-US" unless a
/// test says otherwise.
function stubNavigatorLanguage(tag: string) {
  Object.defineProperty(navigator, "language", { configurable: true, value: tag });
}

beforeEach(() => {
  localStorage.clear();
  stubNavigatorLanguage("en-US");
});

describe("dictionaries", () => {
  /// The types already refuse a missing key; this is the same check with the
  /// compiler out of the picture, because `fr` could still be built at runtime
  /// from a bad merge or a stray `delete`.
  it("French carries exactly the keys of English", () => {
    expect(Object.keys(fr).sort()).toEqual(Object.keys(en).sort());
  });

  it("leaves no key empty", () => {
    expect(Object.entries(fr).filter(([, value]) => value.trim() === "")).toEqual([]);
  });

  /// A `.one` with no `.other` compiles (the cast in `pluralKey` hides it) and
  /// then renders the placeholder key at runtime.
  it("pairs every plural key", () => {
    const singulars = Object.keys(en).filter((key) => key.endsWith(".one"));
    expect(singulars.length).toBeGreaterThan(0);
    for (const key of singulars) {
      expect(en).toHaveProperty(`${key.slice(0, -4)}.other`);
    }
  });
});

describe("language preference", () => {
  it("follows the system by default, French only for a fr tag", () => {
    expect(readLanguagePreference()).toBe("system");
    expect(resolveLanguage("system")).toBe("en");
    stubNavigatorLanguage("fr-CA");
    expect(resolveLanguage("system")).toBe("fr");
    stubNavigatorLanguage("de-DE");
    expect(resolveLanguage("system")).toBe("en");
  });

  it("ignores a stored value that is not a language", () => {
    localStorage.setItem(LANGUAGE_KEY, "es");
    expect(readLanguagePreference()).toBe("system");
  });
});

describe("the Language selector", () => {
  it("switches the rendered language and the document language", async () => {
    render(
      <I18nProvider>
        <SettingsPanel />
      </I18nProvider>,
    );
    expect(screen.getByRole("heading", { name: "Settings" })).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("en");

    await userEvent.selectOptions(screen.getByTestId("language"), "fr");

    expect(screen.getByRole("heading", { name: "Paramètres" })).toBeInTheDocument();
    expect(screen.getByText("Bac à sable")).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("fr");
  });

  it("remembers the choice across a remount", async () => {
    const first = render(
      <I18nProvider>
        <SettingsPanel />
      </I18nProvider>,
    );
    await userEvent.selectOptions(screen.getByTestId("language"), "fr");
    expect(localStorage.getItem(LANGUAGE_KEY)).toBe("fr");
    first.unmount();

    render(
      <I18nProvider>
        <SettingsPanel />
      </I18nProvider>,
    );
    expect(screen.getByRole("heading", { name: "Paramètres" })).toBeInTheDocument();
    expect(screen.getByTestId("language")).toHaveValue("fr");
  });

  /// Settings is where the switch lives; the point of the layer is that the
  /// other screens turn with it.
  it("reaches the other screens", async () => {
    localStorage.setItem(LANGUAGE_KEY, "fr");
    render(
      <I18nProvider>
        <CleanPanel />
      </I18nProvider>,
    );
    await waitFor(() =>
      expect(screen.getByRole("heading", { name: "Nettoyage" })).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: "Analyser" })).toBeInTheDocument();
    expect(screen.getByLabelText("Mode de suppression")).toBeInTheDocument();
  });
});
