import { createApp } from 'vue';
import { createPinia } from 'pinia';
import { i18n, initializeI18n } from '../shared/i18n';
import App from './App.vue';

async function bootstrap(): Promise<void> {
  await initializeI18n();
  createApp(App).use(createPinia()).use(i18n).mount('#app');
}

void bootstrap();
