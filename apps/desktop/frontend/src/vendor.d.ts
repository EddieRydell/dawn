declare module "hoist-non-react-statics" {
  export type NonReactStatics<C, S = Record<string, never>> = C & S;
}

declare module "react-window" {
  export type Align = "auto" | "smart" | "center" | "end" | "start";

  export type CommonProps = {
    innerElementType?: import("react").ElementType;
    outerElementType?: import("react").ElementType;
  };

  export type ListOnItemsRenderedProps = {
    overscanStartIndex: number;
    overscanStopIndex: number;
    visibleStartIndex: number;
    visibleStopIndex: number;
  };

  export type ListOnScrollProps = {
    scrollDirection: "forward" | "backward";
    scrollOffset: number;
    scrollUpdateWasRequested: boolean;
  };

  export class FixedSizeList<P = CommonProps> {
    props: P;
  }
  export class VariableSizeList<P = CommonProps> {
    props: P;
  }
}

interface Window {
  webkitAudioContext?: typeof AudioContext;
}
