import React from "react";

type ErrorBoundaryState = {
  error: Error | null;
};

export class ErrorBoundary extends React.Component<React.PropsWithChildren, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error) {
    console.error(error);
  }

  render() {
    if (this.state.error) {
      return (
        <main className="fatal-error">
          <h1>Dawn hit a frontend error</h1>
          <pre>{this.state.error.stack ?? this.state.error.message}</pre>
        </main>
      );
    }

    return this.props.children;
  }
}
