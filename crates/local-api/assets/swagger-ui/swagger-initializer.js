window.addEventListener('load', function () {
  window.SwaggerUIBundle({
    url: '/openapi.json',
    dom_id: '#swagger-ui',
    presets: [window.SwaggerUIBundle.presets.apis, window.SwaggerUIStandalonePreset],
    layout: 'StandaloneLayout',
    deepLinking: true,
    filter: true,
    persistAuthorization: false,
    validatorUrl: null,
    supportedSubmitMethods: ['get', 'post'],
  });
});
