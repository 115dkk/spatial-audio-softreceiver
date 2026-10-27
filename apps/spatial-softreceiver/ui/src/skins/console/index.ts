// The console skin (ADR 0041, the console's law): the engine on this PC
// observed as an operated system. Health first, the processing flow as a
// map, every number with its unit and basis, every action also a command.
// The behaviour behind the controls is the shared controller's; this skin
// only draws (./shell.ts) with its one stylesheet.

import './console.css';
import type { Api } from '../../api';
import { mountWith } from '../controller';
import type { App, AppOptions } from '../types';
import { createConsole } from './shell';

export function mount(root: HTMLElement, api: Api, options: AppOptions = {}): App {
  const skin = createConsole(root);
  return mountWith(root, api, options, { render: skin.render });
}
