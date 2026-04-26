import { Module } from "@nestjs/common";
import { RetirementsController } from "./retirements.controller";
import { RetirementsService } from "./retirements.service";
import { RetirementIndexerService } from "./retirement-indexer.service";
import { PrismaService } from "../prisma.service";
import { AuthModule } from "../auth/auth.module";
import { QueueModule } from "../queue/queue.module";

@Module({
  imports: [AuthModule, QueueModule],
  controllers: [RetirementsController],
  providers: [RetirementsService, RetirementIndexerService, PrismaService],
  exports: [RetirementIndexerService],
})
export class RetirementsModule {}
