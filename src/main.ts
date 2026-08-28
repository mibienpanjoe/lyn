import { mount } from 'svelte';

import App from './App.svelte';
import './styles.css';

const target = document.getElementById('app');

if (!target) {
  throw new Error('Lyn application root was not found');
}

const app = mount(App, { target });

export default app;
