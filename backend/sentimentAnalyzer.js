// backend/sentimentAnalyzer.js
const { TwitterApi } = require('twitter-api-v2');
const TelegramBot = require('node-telegram-bot-api');
const { exec } = require('child_process');
const util = require('util');
const execPromise = util.promisify(exec);
const { SentimentAnalyzer } = require('natural');
const { pipeline } = require('@huggingface/transformers');
const { logger } = require('./utils/logger');
require('dotenv').config();

class SentimentAnalyzerModule {
    constructor() {
        this.twitterClient = new TwitterApi(process.env.TWITTER_BEARER_TOKEN);
        this.telegramBot = new TelegramBot(process.env.TELEGRAM_BOT_TOKEN, { polling: true });
        this.analyzer = new SentimentAnalyzer('English', 'PorterStemmer', 'afinn');
        this.sentimentScores = new Map();
        this.classifier = null;
        this.initClassifier();
    }

    async initClassifier() {
        try {
            this.classifier = await pipeline('sentiment-analysis', 'distilbert-base-uncased-finetuned-sst-2-english');
            logger.info('Hugging Face Transformers classifier initialized');
        } catch (error) {
            logger.error(`Error initializing classifier: ${error.message}`);
            throw error;
        }
    }

    async analyzeWithBERT(text) {
        try {
            const { stdout } = await execPromise(`python sentiment.py "${text.replace(/"/g, '\\"')}"`);
            const result = JSON.parse(stdout);
            return result.label === 'POSITIVE' ? result.score : -result.score;
        } catch (error) {
            logger.error(`Error analyzing sentiment with BERT: ${error.message}`);
            return 0;
        }
    }

    async analyzeTwitterSentiment(tokenSymbol) {
        try {
            const tweets = await this.twitterClient.v2.search(`$${tokenSymbol} -is:retweet lang:en`).fetch();
            let totalScore = 0;
            let count = 0;

            for await (const tweet of tweets) {
                const score = this.analyzer.getSentiment(tweet.text.split(' '));
                totalScore += score;
                count++;
            }

            const averageScore = count > 0 ? totalScore / count : 0;
            this.sentimentScores.set(tokenSymbol, averageScore);
            logger.info(`Twitter sentiment for ${tokenSymbol}: ${averageScore}`);
            return averageScore;
        } catch (error) {
            logger.error(`Error analyzing Twitter sentiment for ${tokenSymbol}: ${error.message}`);
            return 0;
        }
    }

    async analyzeTelegramSentiment(tokenSymbol) {
        try {
            let totalScore = 0;
            let count = 0;

            this.telegramBot.on('message', (msg) => {
                if (msg.text && msg.text.toLowerCase().includes(tokenSymbol.toLowerCase())) {
                    const score = this.analyzer.getSentiment(msg.text.split(' '));
                    totalScore += score;
                    count++;
                }
            });

            await new Promise(resolve => setTimeout(resolve, 5000)); // Chờ 5 giây để thu thập tin nhắn
            const averageScore = count > 0 ? totalScore / count : 0;
            this.sentimentScores.set(tokenSymbol, averageScore);
            logger.info(`Telegram sentiment for ${tokenSymbol}: ${averageScore}`);
            return averageScore;
        } catch (error) {
            logger.error(`Error analyzing Telegram sentiment for ${tokenSymbol}: ${error.message}`);
            return 0;
        }
    }

    async getSentimentScore(tokenSymbol) {
        const twitterScore = await this.analyzeTwitterSentiment(tokenSymbol);
        const telegramScore = await this.analyzeTelegramSentiment(tokenSymbol);
        const combinedScore = (twitterScore + telegramScore) / 2;
        return combinedScore;
    }

    async sendSentimentToMobile(tokenSymbol, score) {
        // Giả lập gửi qua WebSocket hoặc API đến mobile
        logger.info(`Sending sentiment score for ${tokenSymbol} to mobile: ${score}`);
    }
}

module.exports = SentimentAnalyzerModule;