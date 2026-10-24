import {Composition} from 'remotion';

// S stage only: no storyboard, product content, preview or video is authored.
const EmptyComposition = () => null;

export const RemotionRoot = () => (
  <>
    <Composition
      id="ZhIntro"
      component={EmptyComposition}
      durationInFrames={1}
      fps={30}
      width={1920}
      height={1080}
    />
    <Composition
      id="EnIntro"
      component={EmptyComposition}
      durationInFrames={1}
      fps={30}
      width={1920}
      height={1080}
    />
  </>
);
