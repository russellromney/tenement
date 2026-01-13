const Fastify = require('fastify');

const PORT = process.env.PORT || 3000;
const NODE_ENV = process.env.NODE_ENV || 'development';

const app = Fastify({ logger: true });

// Health check endpoint
app.get('/health', async () => {
  return { status: 'ok' };
});

// Root endpoint
app.get('/', async () => {
  return {
    message: 'Hello from Fastify!',
    env: NODE_ENV,
    port: PORT
  };
});

// Example endpoint
app.get('/users/:id', async (request) => {
  return {
    id: request.params.id,
    name: `User ${request.params.id}`
  };
});

// Start server
const start = async () => {
  try {
    await app.listen({ port: PORT, host: '127.0.0.1' });
    console.log(`Server listening on port ${PORT}`);
  } catch (err) {
    app.log.error(err);
    process.exit(1);
  }
};

start();
