import type { ReactElement, ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";
import { render } from "@testing-library/react";

export function renderWithRouter(
  ui: ReactElement,
  { route = "/" }: { route?: string } = {},
) {
  window.history.replaceState(null, "", route);
  return render(<MemoryRouter initialEntries={[route]}>{ui}</MemoryRouter>);
}

export function Wrapper({ children, route = "/" }: { children: ReactNode; route?: string }) {
  return <MemoryRouter initialEntries={[route]}>{children}</MemoryRouter>;
}
