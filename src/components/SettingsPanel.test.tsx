import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import whatsNew from "@/generated/whats-new.json";
import { SettingsPanel } from "./SettingsPanel";

describe("SettingsPanel", () => {
  it("shows the application name, version and stance", () => {
    render(<SettingsPanel />);
    expect(screen.getByTestId("app-version")).toHaveTextContent(whatsNew.version);
    expect(screen.getByTestId("about")).toHaveTextContent(
      "Open source, MIT. No network access, no telemetry.",
    );
    expect(screen.getByTestId("about")).toHaveTextContent("github.com/CaseReed/wincleaner");
  });

  it("renders the changelog body as plain text under the version heading", () => {
    render(<SettingsPanel />);
    expect(
      screen.getByRole("heading", { name: `What's new in ${whatsNew.version}` }),
    ).toBeInTheDocument();
    const body = screen.getByTestId("whats-new");
    expect(body).toHaveTextContent("Winapp2");
    // Plain text only: the bullets are literal "- ", and no markup was parsed.
    expect(body.textContent).toBe(whatsNew.body);
    expect(body.querySelector("a")).toBeNull();
    expect(body.querySelector("ul")).toBeNull();
  });

  it("states the phase-1 update situation without offering a control", () => {
    render(<SettingsPanel />);
    const updates = screen.getByTestId("updates-placeholder");
    expect(updates).toHaveTextContent("Automatic update checks are not available yet.");
    expect(updates).toHaveTextContent("WinCleaner never contacts the network.");
    expect(screen.queryByRole("switch")).toBeNull();
    expect(screen.queryByRole("button", { name: /check for updates/i })).toBeNull();
  });

  it("credits the MIT licence, Winapp2 and the bundled assets", () => {
    render(<SettingsPanel />);
    const notices = screen.getByTestId("notices");
    expect(notices).toHaveTextContent("MIT");
    expect(notices).toHaveTextContent("Community rules from Winapp2 (CC-BY-SA 4.0)");
    expect(notices).toHaveTextContent("github.com/MoscaDotTo/Winapp2");
    expect(notices).toHaveTextContent(/Fonts and icons are bundled/);
  });
});
