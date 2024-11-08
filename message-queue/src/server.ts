// Imports
import { json, urlencoded } from 'body-parser';
import cors from 'cors';
import express, { Application, NextFunction, Request, Response } from 'express';

// Create application
const app: Application = express();

// Application config
app.use(cors());

app.use(json({ limit: '50mb' }));

app.use(urlencoded({ extended: true, limit: '50mb' }));

// Simple in-memory queue
const messageQueue: {
    [key: string]: any;
}[] = [];

// Routes
app.post('/enqueue', (req, res) => {
    const { message } = req.body;

    if (!message) {
        return res.status(400).send({ error: 'Message is required' });
    }

    messageQueue.push(message);
    console.log(`Message added to queue: ${message}`);
    res.status(200).send({ success: 'Message enqueued' });
});


// Function to process messages from the queue
const processQueue = () => {
    if (messageQueue.length > 0) {
        const message = messageQueue.shift(); // Remove the first message
        console.log(`Processing message: ${message}`);
    } else {
        console.log('No messages to process');
    }
};

// process the queue every 5 seconds
setInterval(processQueue, 5000);

// Health check
app.get('/healthz', (_req, res) => {
    res.status(200).json({
        data: 'ok'
    });
});

// Catch all
app.use('*', (req, res) => {
    res.status(404).json({
        data: 'Not Found'
    });
});

const PORT = 8006;

app.listen(PORT, () => {
    console.log(`MQ is running on port ${PORT}`);
});