import {Composition,Still} from 'remotion';
import {Film,StyleFrame} from './scenes/Film';

export const RemotionRoot = () => <>
 <Composition id="NovaOfficialZh" component={Film} durationInFrames={7200} fps={60} width={1920} height={1080} defaultProps={{locale:'zh-CN' as const}}/>
 <Composition id="NovaOfficialEn" component={Film} durationInFrames={7200} fps={60} width={1920} height={1080} defaultProps={{locale:'en-US' as const}}/>
 <Composition id="NovaProof" component={Film} durationInFrames={600} fps={60} width={1920} height={1080} defaultProps={{locale:'zh-CN' as const}}/>
 <Still id="StyleHero" component={StyleFrame} width={1920} height={1080} defaultProps={{kind:'hero' as const}}/>
 <Still id="StyleFeature" component={StyleFrame} width={1920} height={1080} defaultProps={{kind:'feature' as const}}/>
 <Still id="StyleOutro" component={StyleFrame} width={1920} height={1080} defaultProps={{kind:'outro' as const}}/>
</>;
