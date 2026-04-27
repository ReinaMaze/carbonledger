import { NestFactory } from "@nestjs/core";
import { AppModule } from "./app.module";
import { LoggerService } from "./logger/logger.service";
import { LoggingInterceptor } from "./logger/logging.interceptor";

async function bootstrap() {
  const app = await NestFactory.create(AppModule, { bufferLogs: true });

  const logger = app.get(LoggerService);
  app.useLogger(logger);
  app.useGlobalInterceptors(new LoggingInterceptor(logger));

  app.setGlobalPrefix("api/v1");
  app.enableCors({ origin: process.env.FRONTEND_URL });

  const httpAdapter = app.getHttpAdapter();
  httpAdapter.get("/health", (_req: any, res: any) => {
    res.status(200).json({ status: "ok", timestamp: new Date().toISOString() });
  });

  await app.listen(process.env.PORT ?? 3001);
}
bootstrap();
